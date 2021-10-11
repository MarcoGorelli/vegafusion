use crate::proto::gen::expression::MemberExpression;
use crate::expression::ast::expression::ExpressionTrait;
use std::fmt::{Display, Formatter};

impl MemberExpression {
    pub fn member_binding_power() -> (f64, f64) {
        // See https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Operators/Operator_Precedence
        //  - left-to-right operators have larger number to the right
        //  - right-to-left have larger number to the left
        (20.0, 20.5)
    }
}

impl ExpressionTrait for MemberExpression {
    fn binding_power(&self) -> (f64, f64) {
        Self::member_binding_power()
    }
}

impl Display for MemberExpression {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let object_rhs_bp = self.object.as_ref().unwrap().binding_power().1;

        // Write object, using binding power to determine whether to wrap it in parenthesis
        if object_rhs_bp < Self::member_binding_power().0 {
            write!(f, "({})", self.object.as_ref().unwrap())?;
        } else {
            write!(f, "{}", self.object.as_ref().unwrap())?;
        }

        // Write property
        if self.computed {
            write!(f, "[{}]", self.property.as_ref().unwrap())
        } else {
            write!(f, ".{}", self.property.as_ref().unwrap())
        }
    }
}
