use crate::proto::gen::tasks::{TaskGraph, Task, Variable, TaskNode, OutgoingEdge, IncomingEdge};
use crate::task_graph::scope::TaskScope;
use crate::error::{Result, ResultWithContext, ToExternalError, VegaFusionError};
use std::collections::HashMap;
use petgraph::graph::NodeIndex;
use petgraph::algo::toposort;
use petgraph::Direction;
use petgraph::prelude::EdgeRef;
use itertools::Itertools;
use crate::proto::gen::transforms::{TransformPipeline, Transform, Extent};
use crate::proto::gen::transforms::transform::TransformKind;
use crate::task_graph::task_value::TaskValue;
use crate::data::scalar::ScalarValue;
use crate::proto::gen::tasks::task::TaskKind;
use crate::proto::gen::tasks::task_value::Data;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};


struct PetgraphEdge { output_var: Option<Variable>, propagate: bool }

pub type ScopedVariable = (Variable, Vec<u32>);
pub type NodeValueIndex = (usize, Option<usize>);

impl TaskGraph {
    pub fn new(tasks: Vec<Task>, task_scope: &TaskScope) -> Result<Self> {

        let mut graph: petgraph::graph::DiGraph<ScopedVariable, PetgraphEdge> = petgraph::graph::DiGraph::new();
        let mut tasks_map: HashMap<ScopedVariable, (NodeIndex, Task)> = HashMap::new();

        // Add graph nodes
        for task in tasks {
            // Add scope variable
            let scoped_var = (task.variable().clone(), task.scope.clone());
            let node_index = graph.add_node(scoped_var.clone());
            tasks_map.insert(scoped_var, (node_index, task));
        }

        // Resolve and add edges
        for (node_index, task) in tasks_map.values() {
            let usage_scope = task.scope();
            for input_var in task.input_vars() {
                let resolved = task_scope.resolve_scope(&input_var.var, usage_scope)?;
                let input_scoped_var = (resolved.var.clone(), resolved.scope.clone());
                let (input_node_index, _) = tasks_map.get(&input_scoped_var).with_context(
                    || format!("No variable {:?} with scope {:?}", input_scoped_var.0, input_scoped_var.1)
                )?;

                // Add graph edge
                if input_node_index != node_index {
                    // If a task depends on information generated by the task,that will be handled
                    // internally to the task. So we avoid making a cycle
                    graph.add_edge(
                        input_node_index.clone(),
                        node_index.clone(),
                        PetgraphEdge {
                            output_var: resolved.output_var.clone(),
                            propagate: input_var.propagate,
                        }
                    );
                }
            }
        }

        // Create mapping from toposorted node_index to the final linear node index
        let toposorted: Vec<NodeIndex> = match toposort(&graph, None) {
            Err(err) => return Err(VegaFusionError::internal(
                &format!("failed to sort dependency graph topologically: {:?}", err))
            ),
            Ok(toposorted) => toposorted
        };

        let toposorted_node_indexes: HashMap<NodeIndex, usize> = toposorted.iter().enumerate().map(
            |(sorted_index, node_index)| (*node_index, sorted_index)
        ).collect();

        // Create linear vec of TaskNodes, with edges as sorted index references to nodes
        let task_nodes = toposorted.iter().map(|node_index| {
            let scoped_var = graph.node_weight(*node_index).unwrap();
            let (_, task) = tasks_map.get(scoped_var).unwrap();

            // Collect outgoing node indexes
            let outgoing_node_ids: Vec<_> = graph.edges_directed(*node_index, Direction::Outgoing).map(
                |edge| edge.target()
            ).collect();

            let outgoing: Vec<_> = outgoing_node_ids.iter().map(
                |node_index| {
                    let sorted_index = *toposorted_node_indexes.get(node_index).unwrap() as u32;
                    OutgoingEdge {
                        target: sorted_index,
                        propagate: true,
                    }
                }
            ).collect();
            
            // Collect incoming node indexes
            let incoming_node_ids: Vec<_> = graph.edges_directed(*node_index, Direction::Incoming).map(
                |edge| (edge.source(), &edge.weight().output_var)
            ).collect();

            // Sort incoming nodes to match order expected by the task
            let incoming_vars: HashMap<_, _> = incoming_node_ids.iter().map(|(node_index, output_var)| {
                let var = graph.node_weight(*node_index).unwrap().0.clone();
                (var, (node_index, output_var.clone()))
            }).collect();

            let incoming: Vec<_> = task.input_vars().iter().filter_map(|var| {
                let (node_index, output_var) = *incoming_vars.get(&var.var)?;
                let sorted_index = *toposorted_node_indexes.get(node_index).unwrap() as u32;

                if let Some(output_var) = output_var {
                    let weight = graph.node_weight(*node_index).unwrap();
                    let (_, input_task) = tasks_map.get(weight).unwrap();

                    let output_index = match input_task.output_vars().iter().position(|v| v == output_var) {
                        Some(output_index) => output_index,
                        None => return Some(Err(VegaFusionError::internal("Failed to find output variable")))
                    };

                    Some(Ok(IncomingEdge {
                        source: sorted_index,
                        output: Some(output_index as u32)
                    }))
                } else {
                    Some(Ok(IncomingEdge {
                        source: sorted_index,
                        output: None
                    }))
                }
            }).collect::<Result<Vec<_>>>()?;

            Ok(TaskNode {
                task: Some(task.clone()),
                incoming,
                outgoing,
                id_fingerprint: 0,
                state_fingerprint: 0
            })
        }).collect::<Result<Vec<_>>>()?;

        let mut this = Self {
            nodes: task_nodes
        };

        this.init_identity_fingerprints()?;
        this.update_state_fingerprints()?;

        Ok(this)
    }

    pub fn build_mapping(&self) -> HashMap<ScopedVariable, NodeValueIndex> {
        let mut mapping: HashMap<ScopedVariable, (usize, Option<usize>)> = Default::default();
        for (node_index, node) in self.nodes.iter().enumerate() {
            let task = node.task();
            let scope = task.scope.clone();
            let scoped_var = (task.variable().clone(), task.scope.clone());
            mapping.insert(scoped_var, (node_index, None));

            for (output_index, output_var) in task.output_vars().into_iter().enumerate() {
                let scope_output_var = (output_var, task.scope.clone());
                mapping.insert(scope_output_var, (node_index, Some(output_index)));
            }
        }
        mapping
    }

    fn init_identity_fingerprints(&mut self) -> Result<()> {
        // Compute new identity fingerprints
        let mut id_fingerprints: Vec<u64> = Vec::with_capacity(self.nodes.len());
        for (i, node) in self.nodes.iter().enumerate() {
            let task = node.task();
            let mut hasher = deterministic_hash::DeterministicHasher::new(DefaultHasher::new());

            if let TaskKind::Value(value) = task.task_kind() {
                // Only hash the distinction between Scalar and Table, not the value itself.
                // The state fingerprint takes the value into account.
                task.variable().hash(&mut hasher);
                task.scope.hash(&mut hasher);
                match value.data.as_ref().unwrap() {
                    Data::Scalar(_) => "scalar".hash(&mut hasher),
                    Data::Table(_) => "data".hash(&mut hasher)
                }
            } else {
                // Include id_fingerprint of parents in the hash
                for parent_index in self.parent_indices(i)? {
                    id_fingerprints[parent_index].hash(&mut hasher);
                }

                // Include current task in hash
                task.hash(&mut hasher)
            }

            id_fingerprints.push(hasher.finish());
        }

        // Apply fingerprints
        self.nodes.iter_mut().zip(id_fingerprints).map(|(node, fingerprint)| {
            node.id_fingerprint = fingerprint;
        });

        Ok(())
    }

    /// Update state finger prints of nodes, and return indices of nodes that were updated
    fn update_state_fingerprints(&mut self) -> Result<Vec<usize>> {
        // Compute new identity fingerprints
        let mut state_fingerprints: Vec<u64> = Vec::with_capacity(self.nodes.len());
        for (i, node) in self.nodes.iter().enumerate() {
            let task = node.task();
            let mut hasher = deterministic_hash::DeterministicHasher::new(DefaultHasher::new());

            if matches!(task.task_kind(), TaskKind::Value(_)) {
                // Hash the task with inline TaskValue
                task.hash(&mut hasher);
            } else {
                // Include state fingerprint of parents in the hash
                for parent_index in self.parent_indices(i)? {
                    state_fingerprints[parent_index].hash(&mut hasher);
                }

                // Include id fingerprint of current task
                node.id_fingerprint.hash(&mut hasher);
            }

            state_fingerprints.push(hasher.finish());
        }

        // Apply fingerprints
        let updated: Vec<_> = self.nodes.iter_mut().zip(state_fingerprints).enumerate().filter_map(
            |(node_index, (node, fingerprint))| {
                if node.state_fingerprint != fingerprint {
                    node.state_fingerprint = fingerprint;
                    Some(node_index)
                } else {
                    None
                }
            }).collect();

        Ok(updated)
    }

    pub fn parent_nodes(&self, node_index: usize) -> Result<Vec<&TaskNode>> {
        let node = self.nodes.get(node_index).with_context(
            || format!("Node index {} out of bounds", node_index)
        )?;
        Ok(node.incoming.iter().map(|edge| {
            self.nodes.get(edge.source as usize).unwrap()
        }).collect())
    }

    pub fn parent_indices(&self, node_index: usize) -> Result<Vec<usize>> {
        let node = self.nodes.get(node_index).with_context(
            || format!("Node index {} out of bounds", node_index)
        )?;
        Ok(node.incoming.iter().map(|edge| {
            edge.source as usize
        }).collect())
    }

    pub fn child_nodes(&self, node_index: usize) -> Result<Vec<&TaskNode>> {
        let node = self.nodes.get(node_index).with_context(
            || format!("Node index {} out of bounds", node_index)
        )?;
        Ok(node.outgoing.iter().map(|edge| {
            self.nodes.get(edge.target as usize).unwrap()
        }).collect())
    }

    pub fn child_indices(&self, node_index: usize) -> Result<Vec<usize>> {
        let node = self.nodes.get(node_index).with_context(
            || format!("Node index {} out of bounds", node_index)
        )?;
        Ok(node.outgoing.iter().map(|edge| {
            edge.target as usize
        }).collect())
    }

    pub fn node(&self, node_index: usize) -> Result<&TaskNode> {
        self.nodes.get(node_index).with_context(
            || format!("Node index {} out of bounds", node_index)
        )
    }
}


#[test]
fn try_it() {
    let mut task_scope = TaskScope::new();
    task_scope.add_variable(&Variable::new_signal("url"), Default::default());
    task_scope.add_variable(&Variable::new_data("url_datasetA"), Default::default());
    task_scope.add_variable(&Variable::new_data("datasetA"), Default::default());
    task_scope.add_data_signal("datasetA", "my_extent", Default::default());

    let tasks = vec![
        Task::new_value(
            Variable::new_signal("url"),
            Default::default(),
            TaskValue::Scalar(ScalarValue::from("https://raw.githubusercontent.com/vega/vega-datasets/master/data/penguins.json")),
        ),
        Task::new_scan_url(Variable::new_data("url_datasetA"), Default::default(), ScanUrlTask {
            url: Some(Variable::new_signal("url")),
            batch_size: 1024,
            format_type: None
        }),
        Task::new_transforms(Variable::new_data("datasetA"), Default::default(), TransformsTask {
            source: "url_datasetA".to_string(),
            pipeline: Some(TransformPipeline { transforms: vec![
                Transform { transform_kind: Some(TransformKind::Extent(Extent {
                    field: "Beak Length (mm)".to_string(),
                    signal: Some("my_extent".to_string()),
                })) }
            ] })
        })
    ];

    let graph = TaskGraph::new(tasks, task_scope).unwrap();
    println!("graph:\n{:#?}", graph);
}


impl TaskNode {
    pub fn task(&self) -> &Task {
        self.task.as_ref().unwrap()
    }
}