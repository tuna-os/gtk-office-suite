//! Collection of common expressions.

use gtk::{glib, glib::closure};
use secular::normalized_lower_lay_string;

/// Returns an expression that is the and’ed result of the given boolean
/// expressions.
pub(crate) fn and(
    a_expr: impl AsRef<gtk::Expression>,
    b_expr: impl AsRef<gtk::Expression>,
) -> gtk::ClosureExpression {
    gtk::ClosureExpression::new::<bool>(
        &[a_expr.as_ref(), b_expr.as_ref()],
        closure!(|_: Option<glib::Object>, a: bool, b: bool| { a && b }),
    )
}

/// Returns an expression that is the or’ed result of the given boolean
/// expressions.
pub(crate) fn or(
    a_expr: impl AsRef<gtk::Expression>,
    b_expr: impl AsRef<gtk::Expression>,
) -> gtk::ClosureExpression {
    gtk::ClosureExpression::new::<bool>(
        &[a_expr.as_ref(), b_expr.as_ref()],
        closure!(|_: Option<glib::Object>, a: bool, b: bool| { a || b }),
    )
}

/// Returns an expression that is the inverted result of the given boolean
/// expression.
pub(crate) fn not<E: AsRef<gtk::Expression>>(a_expr: E) -> gtk::ClosureExpression {
    gtk::ClosureExpression::new::<bool>(
        &[a_expr],
        closure!(|_: Option<glib::Object>, a: bool| { !a }),
    )
}

/// Returns an expression that is the normalized version of the given string
/// expression.
pub(crate) fn normalize_string<E: AsRef<gtk::Expression>>(expr: E) -> gtk::ClosureExpression {
    gtk::ClosureExpression::new::<String>(
        &[expr],
        closure!(|_: Option<glib::Object>, s: &str| { normalized_lower_lay_string(s) }),
    )
}
