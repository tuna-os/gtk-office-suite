//! Commonly-imported traits and types.
//!
//! Modules that contain components can glob import this module to bring all needed types and
//! traits into scope.

pub use crate::relm::component::{
    AsyncComponent, AsyncComponentController, AsyncComponentParts, AsyncComponentSender,
    AsyncController, SimpleAsyncComponent,
};
pub use crate::relm::factory::{
    AsyncFactoryComponent, AsyncFactorySender, AsyncFactoryVecDeque, DynamicIndex,
    FactoryComponent, FactorySender, FactoryVecDeque,
};
pub use crate::relm::{
    Component, ComponentController, ComponentParts, ComponentSender, Controller, RelmApp,
    RelmWidgetExt, SimpleComponent, WidgetRef, WidgetTemplate,
};

pub use adw;
pub use gtk;

