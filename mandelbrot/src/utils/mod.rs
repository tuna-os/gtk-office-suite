//! Collection of common methods and types.

use std::{
    borrow::Cow,
    cell::{Cell, OnceCell, RefCell},
    fmt, fs,
    io::{self, Write},
    ops::Deref,
    path::{Path, PathBuf},
    rc::{Rc, Weak},
    sync::{Arc, LazyLock, Mutex},
};

use adw::prelude::*;
use futures_channel::oneshot;
use futures_util::future::BoxFuture;
use gtk::{gio, glib};
use regex::Regex;
use tempfile::NamedTempFile;
use tokio::task::{AbortHandle, JoinHandle};
use tracing::error;

pub(crate) mod expression;
mod expression_list_model;
mod fixed_selection;
mod grouping_list_model;
pub(crate) mod key_bindings;
mod location;
mod macros;
pub(crate) mod matrix;
pub(crate) mod media;
pub(crate) mod notifications;
mod placeholder_object;
mod single_item_list_model;
pub(crate) mod sourceview;
pub(crate) mod string;
mod template_callbacks;
pub(crate) mod toast;

pub(crate) use self::{
    expression_list_model::ExpressionListModel,
    fixed_selection::FixedSelection,
    grouping_list_model::*,
    location::{Location, LocationError, LocationExt},
    placeholder_object::PlaceholderObject,
    single_item_list_model::SingleItemListModel,
    template_callbacks::TemplateCallbacks,
};
use crate::{PROFILE, RUNTIME};

/// The type of data.
#[derive(Debug, Clone, Copy)]
pub(crate) enum DataType {
    /// Data that should not be deleted.
    Persistent,
    /// Cache that can be deleted freely.
    Cache,
}

impl DataType {
    /// The path of the directory where data should be stored, depending on this
    /// type.
    pub(crate) fn dir_path(self) -> PathBuf {
        let mut path = match self {
            DataType::Persistent => glib::user_data_dir(),
            DataType::Cache => glib::user_cache_dir(),
        };
        path.push(PROFILE.dir_name().as_ref());

        path
    }
}

/// Replace variables in the given string with the given dictionary.
///
/// The expected format to replace is `{name}`, where `name` is the first string
/// in the dictionary entry tuple.
pub(crate) fn freplace<'a>(s: &'a str, args: &[(&str, &str)]) -> Cow<'a, str> {
    let mut s = Cow::Borrowed(s);

    for (k, v) in args {
        s = Cow::Owned(s.replace(&format!("{{{k}}}"), v));
    }

    s
}

/// Regex that matches a string that only includes emojis.
pub(crate) static EMOJI_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        ^
        [\p{White_Space}\p{Emoji_Component}]*
        [\p{Emoji}--\p{Decimal_Number}]+
        [\p{White_Space}\p{Emoji}\p{Emoji_Component}--\p{Decimal_Number}]*
        $
        # That string is made of at least one emoji, except digits, possibly more,
        # possibly with modifiers, possibly with spaces, but nothing else
        ",
    )
    .unwrap()
});

/// Inner to manage a bound object.
#[derive(Debug)]
struct BoundObjectInner<T: ObjectType> {
    obj: T,
    signal_handler_ids: Vec<glib::SignalHandlerId>,
}

/// Wrapper to manage a bound object.
///
/// This keeps a strong reference to the object.
#[derive(Debug)]
pub struct BoundObject<T: ObjectType> {
    inner: RefCell<Option<BoundObjectInner<T>>>,
}

impl<T: ObjectType> BoundObject<T> {
    /// Creates a new empty `BoundObject`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the given object and signal handlers IDs.
    ///
    /// Calls `disconnect_signals` first to drop the previous strong reference
    /// and disconnect the previous signal handlers.
    pub(crate) fn set(&self, obj: T, signal_handler_ids: Vec<glib::SignalHandlerId>) {
        self.disconnect_signals();

        let inner = BoundObjectInner {
            obj,
            signal_handler_ids,
        };

        self.inner.replace(Some(inner));
    }

    /// Get the object, if any.
    pub fn obj(&self) -> Option<T> {
        self.inner.borrow().as_ref().map(|inner| inner.obj.clone())
    }

    /// Disconnect the signal handlers and drop the strong reference.
    pub fn disconnect_signals(&self) {
        if let Some(inner) = self.inner.take() {
            for signal_handler_id in inner.signal_handler_ids {
                inner.obj.disconnect(signal_handler_id);
            }
        }
    }
}

impl<T: ObjectType> Default for BoundObject<T> {
    fn default() -> Self {
        Self {
            inner: Default::default(),
        }
    }
}

impl<T: ObjectType> Drop for BoundObject<T> {
    fn drop(&mut self) {
        self.disconnect_signals();
    }
}

impl<T: IsA<glib::Object> + glib::HasParamSpec> glib::property::Property for BoundObject<T> {
    type Value = Option<T>;
}

impl<T: IsA<glib::Object>> glib::property::PropertyGet for BoundObject<T> {
    type Value = Option<T>;

    fn get<R, F: Fn(&Self::Value) -> R>(&self, f: F) -> R {
        f(&self.obj())
    }
}

/// Wrapper to manage a bound object.
///
/// This keeps a weak reference to the object.
#[derive(Debug)]
pub struct BoundObjectWeakRef<T: ObjectType> {
    weak_obj: glib::WeakRef<T>,
    signal_handler_ids: RefCell<Vec<glib::SignalHandlerId>>,
}

impl<T: ObjectType> BoundObjectWeakRef<T> {
    /// Creates a new empty `BoundObjectWeakRef`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the given object and signal handlers IDs.
    ///
    /// Calls `disconnect_signals` first to remove the previous weak reference
    /// and disconnect the previous signal handlers.
    pub(crate) fn set(&self, obj: &T, signal_handler_ids: Vec<glib::SignalHandlerId>) {
        self.disconnect_signals();

        self.weak_obj.set(Some(obj));
        self.signal_handler_ids.replace(signal_handler_ids);
    }

    /// Get a strong reference to the object.
    pub fn obj(&self) -> Option<T> {
        self.weak_obj.upgrade()
    }

    /// Disconnect the signal handlers and drop the weak reference.
    pub fn disconnect_signals(&self) {
        let signal_handler_ids = self.signal_handler_ids.take();

        if let Some(obj) = self.weak_obj.upgrade() {
            for signal_handler_id in signal_handler_ids {
                obj.disconnect(signal_handler_id);
            }
        }

        self.weak_obj.set(None);
    }
}

impl<T: ObjectType> Default for BoundObjectWeakRef<T> {
    fn default() -> Self {
        Self {
            weak_obj: Default::default(),
            signal_handler_ids: Default::default(),
        }
    }
}

impl<T: ObjectType> Drop for BoundObjectWeakRef<T> {
    fn drop(&mut self) {
        self.disconnect_signals();
    }
}

impl<T: IsA<glib::Object> + glib::HasParamSpec> glib::property::Property for BoundObjectWeakRef<T> {
    type Value = Option<T>;
}

impl<T: IsA<glib::Object>> glib::property::PropertyGet for BoundObjectWeakRef<T> {
    type Value = Option<T>;

    fn get<R, F: Fn(&Self::Value) -> R>(&self, f: F) -> R {
        f(&self.obj())
    }
}

/// Wrapper to manage a bound construct-only object.
///
/// This keeps a strong reference to the object.
#[derive(Debug)]
pub struct BoundConstructOnlyObject<T: ObjectType> {
    obj: OnceCell<T>,
    signal_handler_ids: RefCell<Vec<glib::SignalHandlerId>>,
}

impl<T: ObjectType> BoundConstructOnlyObject<T> {
    /// Creates a new empty `BoundConstructOnlyObject`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the given object and signal handlers IDs.
    ///
    /// Panics if the object was already set.
    pub(crate) fn set(&self, obj: T, signal_handler_ids: Vec<glib::SignalHandlerId>) {
        self.obj.set(obj).unwrap();
        self.signal_handler_ids.replace(signal_handler_ids);
    }

    /// Get a strong reference to the object.
    ///
    /// Panics if the object has not been set yet.
    pub fn obj(&self) -> &T {
        self.obj.get().unwrap()
    }
}

impl<T: ObjectType> Default for BoundConstructOnlyObject<T> {
    fn default() -> Self {
        Self {
            obj: Default::default(),
            signal_handler_ids: Default::default(),
        }
    }
}

impl<T: ObjectType> Drop for BoundConstructOnlyObject<T> {
    fn drop(&mut self) {
        let signal_handler_ids = self.signal_handler_ids.take();

        if let Some(obj) = self.obj.get() {
            for signal_handler_id in signal_handler_ids {
                obj.disconnect(signal_handler_id);
            }
        }
    }
}

impl<T: IsA<glib::Object> + glib::HasParamSpec> glib::property::Property
    for BoundConstructOnlyObject<T>
{
    type Value = T;
}

impl<T: IsA<glib::Object>> glib::property::PropertyGet for BoundConstructOnlyObject<T> {
    type Value = T;

    fn get<R, F: Fn(&Self::Value) -> R>(&self, f: F) -> R {
        f(self.obj())
    }
}

/// Helper type to keep track of ongoing async actions that can succeed in
/// different functions.
///
/// This type can only have one strong reference and many weak references.
///
/// The strong reference should be dropped in the first function where the
/// action succeeds. Then other functions can drop the weak references when
/// they can't be upgraded.
#[derive(Debug)]
pub struct OngoingAsyncAction<T> {
    strong: Rc<AsyncAction<T>>,
}

impl<T> OngoingAsyncAction<T> {
    /// Create a new async action that sets the given value.
    ///
    /// Returns both a strong and a weak reference.
    pub(crate) fn set(value: T) -> (Self, WeakOngoingAsyncAction<T>) {
        let strong = Rc::new(AsyncAction::Set(value));
        let weak = Rc::downgrade(&strong);
        (Self { strong }, WeakOngoingAsyncAction { weak })
    }

    /// Create a new async action that removes a value.
    ///
    /// Returns both a strong and a weak reference.
    pub(crate) fn remove() -> (Self, WeakOngoingAsyncAction<T>) {
        let strong = Rc::new(AsyncAction::Remove);
        let weak = Rc::downgrade(&strong);
        (Self { strong }, WeakOngoingAsyncAction { weak })
    }

    /// Get the inner value, if any.
    pub(crate) fn as_value(&self) -> Option<&T> {
        self.strong.as_value()
    }
}

/// A weak reference to an `OngoingAsyncAction`.
#[derive(Debug, Clone)]
pub struct WeakOngoingAsyncAction<T> {
    weak: Weak<AsyncAction<T>>,
}

impl<T> WeakOngoingAsyncAction<T> {
    /// Whether this async action is still ongoing (i.e. whether the strong
    /// reference still exists).
    pub fn is_ongoing(&self) -> bool {
        self.weak.strong_count() > 0
    }
}

/// An async action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AsyncAction<T> {
    /// An async action is ongoing to set this value.
    Set(T),

    /// An async action is ongoing to remove a value.
    Remove,
}

impl<T> AsyncAction<T> {
    /// Get the inner value, if any.
    pub fn as_value(&self) -> Option<&T> {
        match self {
            Self::Set(value) => Some(value),
            Self::Remove => None,
        }
    }
}

/// A wrapper that requires the tokio runtime to be running when dropped.
#[derive(Debug, Clone)]
pub struct TokioDrop<T>(Option<T>);

impl<T> TokioDrop<T> {
    /// Create a new `TokioDrop` wrapping the given type.
    pub fn new(value: T) -> Self {
        Self(Some(value))
    }
}

impl<T> Deref for TokioDrop<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0
            .as_ref()
            .expect("TokioDrop should always contain a value")
    }
}

impl<T> From<T> for TokioDrop<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T> Drop for TokioDrop<T> {
    fn drop(&mut self) {
        let _guard = RUNTIME.enter();

        if let Some(value) = self.0.take() {
            drop(value);
        }
    }
}

/// The state of a resource that can be loaded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, glib::Enum)]
#[enum_type(name = "LoadingState")]
pub enum LoadingState {
    /// It hasn't been loaded yet.
    #[default]
    Initial,
    /// It is currently loading.
    Loading,
    /// It has been fully loaded.
    Ready,
    /// An error occurred while loading it.
    Error,
}

/// Convert the given checked `bool` to a `GtkAccessibleTristate`.
pub(crate) fn bool_to_accessible_tristate(checked: bool) -> gtk::AccessibleTristate {
    if checked {
        gtk::AccessibleTristate::True
    } else {
        gtk::AccessibleTristate::False
    }
}

/// A wrapper around several sources of files.
#[derive(Debug, Clone)]
pub enum File {
    /// A `GFile`.
    Gio(gio::File),
    /// A temporary file.
    ///
    /// When all strong references to this file are destroyed, the file will be
    /// destroyed too.
    Temp(Arc<NamedTempFile>),
}

impl File {
    /// The path to the file.
    pub(crate) fn path(&self) -> Option<PathBuf> {
        match self {
            Self::Gio(file) => file.path(),
            Self::Temp(file) => Some(file.path().to_owned()),
        }
    }

    /// Get a `GFile` for this file.
    pub(crate) fn as_gfile(&self) -> gio::File {
        match self {
            Self::Gio(file) => file.clone(),
            Self::Temp(file) => gio::File::for_path(file.path()),
        }
    }
}

impl From<gio::File> for File {
    fn from(value: gio::File) -> Self {
        Self::Gio(value)
    }
}

impl From<NamedTempFile> for File {
    fn from(value: NamedTempFile) -> Self {
        Self::Temp(value.into())
    }
}

/// The directory where to put temporary files.
static TMP_DIR: LazyLock<Box<Path>> = LazyLock::new(|| {
    let mut dir = glib::user_runtime_dir();
    dir.push(PROFILE.dir_name().as_ref());
    dir.into_boxed_path()
});

/// Save the given data to a temporary file.
///
/// When all strong references to the returned file are destroyed, the file will
/// be destroyed too.
pub(crate) async fn save_data_to_tmp_file(data: Vec<u8>) -> Result<File, std::io::Error> {
    RUNTIME
        .spawn_blocking(move || {
            let dir = TMP_DIR.as_ref();
            if !dir.exists()
                && let Err(error) = fs::create_dir(dir)
                && !matches!(error.kind(), io::ErrorKind::AlreadyExists)
            {
                return Err(error);
            }

            let mut file = NamedTempFile::new_in(dir)?;
            file.write_all(&data)?;

            Ok(file.into())
        })
        .await
        .expect("task was not aborted")
}

/// A counted reference.
///
/// Can be used to perform some actions when the count is 0 or non-zero.
pub struct CountedRef(Rc<InnerCountedRef>);

struct InnerCountedRef {
    /// The count of the reference
    count: Cell<usize>,
    /// The function to call when the count decreases to zero.
    on_zero: Box<dyn Fn()>,
    /// The function to call when the count increases from zero.
    on_non_zero: Box<dyn Fn()>,
}

impl CountedRef {
    /// Construct a counted reference.
    pub(crate) fn new<F1, F2>(on_zero: F1, on_non_zero: F2) -> Self
    where
        F1: Fn() + 'static,
        F2: Fn() + 'static,
    {
        Self(
            InnerCountedRef {
                count: Default::default(),
                on_zero: Box::new(on_zero),
                on_non_zero: Box::new(on_non_zero),
            }
            .into(),
        )
    }

    /// The current count of the reference.
    pub fn count(&self) -> usize {
        self.0.count.get()
    }
}

impl fmt::Debug for CountedRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CountedRef")
            .field("count", &self.count())
            .finish_non_exhaustive()
    }
}

impl Clone for CountedRef {
    fn clone(&self) -> Self {
        let count = self.count();
        self.0.count.set(count.saturating_add(1));

        if count == 0 {
            (self.0.on_non_zero)();
        }

        Self(self.0.clone())
    }
}

impl Drop for CountedRef {
    fn drop(&mut self) {
        let count = self.count();
        self.0.count.set(count.saturating_sub(1));

        if count == 1 {
            (self.0.on_zero)();
        }
    }
}

/// Extensions trait for types with a `child` property.
pub(crate) trait ChildPropertyExt {
    /// The child of this widget, is any.
    fn child_property(&self) -> Option<gtk::Widget>;

    /// Set the child of this widget.
    fn set_child_property(&self, child: Option<&impl IsA<gtk::Widget>>);

    /// Get the child if it is of the proper type, or construct it with the
    /// given function and set is as the child of this widget before returning
    /// it.
    fn child_or_else<W>(&self, f: impl FnOnce() -> W) -> W
    where
        W: IsA<gtk::Widget>,
    {
        if let Some(child) = self.child_property().and_downcast() {
            child
        } else {
            let child = f();
            self.set_child_property(Some(&child));
            child
        }
    }

    /// Get the child if it is of the proper type, or construct it with its
    /// `Default` implementation and set is as the child of this widget before
    /// returning it.
    fn child_or_default<W>(&self) -> W
    where
        W: IsA<gtk::Widget> + Default,
    {
        self.child_or_else(Default::default)
    }
}

impl<W> ChildPropertyExt for W
where
    W: IsABin,
{
    fn child_property(&self) -> Option<gtk::Widget> {
        self.child()
    }

    fn set_child_property(&self, child: Option<&impl IsA<gtk::Widget>>) {
        self.set_child(child);
    }
}

impl ChildPropertyExt for gtk::ListItem {
    fn child_property(&self) -> Option<gtk::Widget> {
        self.child()
    }

    fn set_child_property(&self, child: Option<&impl IsA<gtk::Widget>>) {
        self.set_child(child);
    }
}

/// Helper trait to implement for widgets that subclass `AdwBin`, to be able to
/// use the `ChildPropertyExt` trait.
///
/// This trait is to circumvent conflicts in Rust's type system, where if we try
/// to implement `ChildPropertyExt for W where W: IsA<adw::Bin>` it complains
/// that the other external types that implement `ChildPropertyExt` might
/// implement `IsA<adw::Bin>` in the future… So instead of reimplementing
/// `ChildPropertyExt` for every type where we need it, which requires to
/// implement two methods, we only implement this which requires nothing.
pub(crate) trait IsABin: IsA<adw::Bin> {}

impl IsABin for adw::Bin {}

/// A wrapper around [`JoinHandle`] that aborts the future if it is dropped
/// before the task ends.
///
/// The main API for this type is [`AbortableHandle::await_task()`].
#[derive(Debug, Default)]
pub(crate) struct AbortableHandle {
    abort_handle: RefCell<Option<AbortHandle>>,
}

impl AbortableHandle {
    /// Await the task of the given `JoinHandle`.
    ///
    /// Aborts the previous task that was running, if any.
    ///
    /// Returns `None` if the task was aborted before completion.
    pub(crate) async fn await_task<T>(&self, join_handle: JoinHandle<T>) -> Option<T> {
        self.abort();

        self.abort_handle.replace(Some(join_handle.abort_handle()));

        let result = join_handle.await.ok();

        self.abort_handle.take();

        result
    }

    /// Abort the current task, if possible.
    pub(crate) fn abort(&self) {
        if let Some(abort_handle) = self.abort_handle.take() {
            abort_handle.abort();
        }
    }
}

impl Drop for AbortableHandle {
    fn drop(&mut self) {
        self.abort();
    }
}

/// Resample the given slice to the given length, using linear interpolation.
///
/// Returns the slice as-is if it is of the correct length. Returns a `Vec` of
/// zeroes if the slice is empty.
pub(crate) fn resample_slice(slice: &[f32], new_len: usize) -> Cow<'_, [f32]> {
    let len = slice.len();

    if len == new_len {
        // The slice has the correct length, return it.
        return Cow::Borrowed(slice);
    }

    if new_len == 0 {
        // We do not need values, return an empty slice.
        return Cow::Borrowed(&[]);
    }

    if len <= 1
        || slice
            .iter()
            .all(|value| (*value - slice[0]).abs() < 0.000_001)
    {
        // There is a single value so we do not need to interpolate, return a `Vec`
        // containing that value.
        let value = slice.first().copied().unwrap_or_default();
        return Cow::Owned(std::iter::repeat_n(value, new_len).collect());
    }

    // We need to interpolate the values.
    let mut result = Vec::with_capacity(new_len);
    let ratio = (len - 1) as f32 / (new_len - 1) as f32;

    for i in 0..new_len {
        let position_abs = i as f32 * ratio;
        let position_before = position_abs.floor();
        let position_after = position_abs.ceil();
        let position_rel = position_abs % 1.0;

        // We are sure that the positions are positive.
        #[allow(clippy::cast_sign_loss)]
        let value_before = slice[position_before as usize];
        #[allow(clippy::cast_sign_loss)]
        let value_after = slice[(position_after as usize).min(slice.len().saturating_sub(1))];

        let value = (1.0 - position_rel) * value_before + position_rel * value_after;
        result.push(value);
    }

    Cow::Owned(result)
}

/// A helper type to wait for a notification that can occur only one time.
///
/// [`OneshotNotifier::listen()`] must be called to initialize it and get a
/// receiver. The receiver must then be `.await`ed and the future will resolve
/// when it is notified.
///
/// The receiver will receive a signal the first time that
/// [`OneshotNotifier::notify_value()`] is called. Further calls to this
/// function will be noops until a new receiver is created.The value to return
/// must implement `Default`, as this is the value that will be sent to the
/// receiver when the notifier is dropped before a value is notified.
///
/// This notifier can be cloned freely and moved between threads.
///
/// It is also possible to share this notifier between tasks to make sure that a
/// single task is running at a time. If [`OneshotNotifier::listen()`] is called
/// while there is already a receiver waiting, it will be notified as if the
/// notifier was dropped.
#[derive(Debug, Clone)]
pub(crate) struct OneshotNotifier<T = ()> {
    /// The context used to identify the notifier in logs.
    context: &'static str,
    /// The sender for the notification signal.
    sender: Arc<Mutex<Option<oneshot::Sender<T>>>>,
}

impl<T> OneshotNotifier<T> {
    /// Get a new `OneshotNotifier` for the given context.
    pub(crate) fn new(context: &'static str) -> Self {
        Self {
            sender: Default::default(),
            context,
        }
    }
}

impl<T> OneshotNotifier<T>
where
    T: Default + Send + 'static,
{
    /// Initialize this `OneshotNotifier` and get a receiver.
    pub(crate) fn listen(&self) -> OneshotNotifierReceiver<T> {
        let (sender, receiver) = oneshot::channel();

        match self.sender.lock() {
            Ok(mut guard) => *guard = Some(sender),
            Err(error) => {
                error!(
                    context = self.context,
                    "Failed to lock oneshot notifier: {error}"
                );
            }
        }

        OneshotNotifierReceiver(receiver)
    }

    /// Notify the receiver with the given value, if any receiver is still
    /// listening.
    pub(crate) fn notify_value(&self, value: T) {
        match self.sender.lock() {
            Ok(mut guard) => {
                if let Some(sender) = guard.take() {
                    let _ = sender.send(value);
                }
            }
            Err(error) => {
                error!(
                    context = self.context,
                    "Failed to lock oneshot notifier: {error}"
                );
            }
        }
    }

    /// Notify the receiver with the default value, if any receiver is still
    /// listening.
    pub(crate) fn notify(&self) {
        self.notify_value(T::default());
    }
}

/// A notification receiver associated to a [`OneshotNotifier`].
///
/// This should be `.await`ed to wait for a notification.
#[derive(Debug)]
pub(crate) struct OneshotNotifierReceiver<T = ()>(oneshot::Receiver<T>);

impl<T> IntoFuture for OneshotNotifierReceiver<T>
where
    T: Default + Send + 'static,
{
    type Output = T;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move { self.0.await.unwrap_or_default() })
    }
}
