use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use crate::spec::data::DataSpec;
use crate::spec::signal::SignalSpec;
use crate::spec::mark::MarkSpec;
use crate::spec::scale::ScaleSpec;
use crate::error::{Result, ResultWithContext, VegaFusionError};
use crate::task_graph::scope::TaskScope;
use crate::spec::visitors::{MakeTaskScopeVisitor, MakeTasksVisitor};
use crate::proto::gen::tasks::Task;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartSpec {
    #[serde(rename = "$schema", default = "default_schema")]
    pub schema: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data: Vec<DataSpec>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signals: Vec<SignalSpec>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<MarkSpec>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scales: Vec<ScaleSpec>,

    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

pub fn default_schema() -> String {
    String::from("https://vega.github.io/schema/vega/v5.json")
}

impl ChartSpec {
    pub fn walk(&self, visitor: &mut dyn ChartVisitor) -> Result<()> {
        // Top-level with empty scope
        let scope: Vec<u32> = Vec::new();
        for data in &self.data {
            visitor.visit_data(data, &scope)?;
        }
        for scale in &self.scales {
            visitor.visit_scale(scale, &scope)?;
        }
        for signal in &self.signals {
            visitor.visit_signal(signal, &scope)?;
        }

        // Child groups
        let mut group_index = 0;
        for mark in &self.marks {
            if mark.type_ == "group" {
                // Add group index to scope
                let mut nested_scope = scope.clone();
                nested_scope.push(group_index);

                visitor.visit_group_mark(mark, &nested_scope)?;
                mark.walk(visitor, &nested_scope)?;
                group_index += 1;
            } else {
                // Keep parent scope
                visitor.visit_non_group_mark(mark, &scope)?;
            }
        }

        Ok(())
    }

    pub fn walk_mut(&mut self, visitor: &mut dyn MutChartVisitor) -> Result<()> {
        // Top-level with empty scope
        let scope: Vec<u32> = Vec::new();
        for data in &mut self.data {
            visitor.visit_data(data, &scope)?;
        }
        for scale in &mut self.scales {
            visitor.visit_scale(scale, &scope)?;
        }
        for signal in &mut self.signals {
            visitor.visit_signal(signal, &scope)?;
        }

        // Child groups
        let mut group_index = 0;
        for mark in &mut self.marks {
            if mark.type_ == "group" {
                // Add group index to scope
                let mut nested_scope = scope.clone();
                nested_scope.push(group_index);

                visitor.visit_group_mark(mark, &nested_scope)?;
                mark.walk_mut(visitor, &nested_scope)?;
                group_index += 1;
            } else {
                // Keep parent scope
                visitor.visit_non_group_mark(mark, &scope)?;
            }
        }

        Ok(())
    }

    pub fn to_task_scope(&self) -> Result<TaskScope> {
        let mut visitor = MakeTaskScopeVisitor::new();
        self.walk(&mut visitor)?;
        Ok(visitor.task_scope)
    }

    pub fn to_tasks(&self) -> Result<Vec<Task>> {
        let mut visitor = MakeTasksVisitor::new();
        self.walk(&mut visitor)?;
        Ok(visitor.tasks)
    }

    pub fn get_group_mut(&mut self, group_index: u32) -> Result<&mut MarkSpec> {
        self.marks
            .iter_mut()
            .filter(|m| m.type_ == "group")
            .nth(group_index as usize)
            .with_context(|| format!("No group with index {}", group_index))
    }

    pub fn get_nested_group_mut(&mut self, path: &[u32]) -> Result<&mut MarkSpec> {
        if path.is_empty() {
            return Err(VegaFusionError::internal("Path may not be empty"))
        }
        let mut group = self.get_group_mut(path[0])?;
        for group_index in &path[1..] {
            group = group.get_group_mut(*group_index)?;
        }
        Ok(group)
    }
}


pub trait ChartVisitor {
    fn visit_data(&mut self, _data: &DataSpec, _scope: &[u32]) -> Result<()> {
        Ok(())
    }
    fn visit_signal(&mut self, _signal: &SignalSpec, _scope: &[u32]) -> Result<()> {
        Ok(())
    }
    fn visit_scale(&mut self, _scale: &ScaleSpec, _scope: &[u32]) -> Result<()> {
        Ok(())
    }
    fn visit_non_group_mark(&mut self, _mark: &MarkSpec, _scope: &[u32]) -> Result<()> {
        Ok(())
    }
    fn visit_group_mark(&mut self, _mark: &MarkSpec, _scope: &[u32]) -> Result<()> {
        Ok(())
    }
}


pub trait MutChartVisitor {
    fn visit_data(&mut self, _data: &mut DataSpec, _scope: &[u32]) -> Result<()> {
        Ok(())
    }
    fn visit_signal(&mut self, _signal: &mut SignalSpec, _scope: &[u32]) -> Result<()> {
        Ok(())
    }
    fn visit_scale(&mut self, _scale: &mut ScaleSpec, _scope: &[u32]) -> Result<()> {
        Ok(())
    }
    fn visit_non_group_mark(&mut self, _mark: &mut MarkSpec, _scope: &[u32]) -> Result<()> {
        Ok(())
    }
    fn visit_group_mark(&mut self, _mark: &mut MarkSpec, _scope: &[u32]) -> Result<()> {
        Ok(())
    }
}
