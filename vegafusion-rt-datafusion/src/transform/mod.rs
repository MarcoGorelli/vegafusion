pub mod filter;

use std::fmt::Debug;
use std::sync::Arc;
use datafusion::dataframe::DataFrame;
use datafusion::scalar::ScalarValue;
use vegafusion_core::error::Result;
use crate::expression::compiler::config::CompilationConfig;
use vegafusion_core::variable::Variable;


pub trait TransformTrait: Send + Sync {
    fn call(
        &self,
        dataframe: Arc<dyn DataFrame>,
        config: &CompilationConfig,
    ) -> Result<(Arc<dyn DataFrame>, Vec<ScalarValue>)>;

    fn input_vars(&self) -> Vec<Variable> {
        Vec::new()
    }

    fn output_signals(&self) -> Vec<String> {
        Vec::new()
    }
}
