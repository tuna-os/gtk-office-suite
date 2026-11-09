use std::time::Duration;

use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{
    gdk, gio, glib,
    glib::{clone, closure, closure_local},
};
use tracing::{debug, error};

use super::{AvatarData, AvatarImage};
use crate::{
    components::{ActionButton, ActionState, AnimatedImagePaintable},
    toast,
    utils::{
        BoundObject, BoundObjectWeakRef, CountedRef, SingleItemListModel, expression,
        media::{
            FrameDimensions,
            image::{IMAGE_QUEUE, ImageError},
        },
    },
};

/// The state of the editable avatar.
#[derive(Debug, Default, Hash, Eq, PartialEq, Clone, Copy, glib::Enum)]
#[repr(u32)]
#[enum_type(name = "EditableAvatarState")]
pub enum EditableAvatarState {
    /// Nothing is currently happening.
    #[default]
    Default = 0,
    /// An edit is in progress.
    EditInProgress = 1,
    /// An edit was successful.
    EditSuccessful = 2,
    // A removal is in progress.
    RemovalInProgress = 3,
}

mod imp {
    use std::{
        cell::{Cell, RefCell},
        sync::LazyLock,
    };

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/avatar/editable.ui")]
    #[properties(wrapper_type = super::EditableAvatar)]
    pub struct EditableAvatar {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        temp_avatar: TemplateChild<adw::Avatar>,
        #[template_child]
        error_img: TemplateChild<gtk::Image>,
        #[template_child]
        button_remove: TemplateChild<ActionButton>,
        #[template_child]
        button_edit: TemplateChild<ActionButton>,
        /// The [`AvatarData`] to display.
        #[property(get, set = Self::set_data, explicit_notify)]
        data: BoundObject<AvatarData>,
        /// The avatar image to watch.
        #[property(get)]
        image: BoundObjectWeakRef<AvatarImage>,
        /// Whether this avatar is changeable.
        #[property(get, set = Self::set_editable, explicit_notify)]
        editable: Cell<bool>,
        /// Whether to prevent the remove button from showing.
        #[property(get, set = Self::set_inhibit_remove, explicit_notify)]
        inhibit_remove: Cell<bool>,
        /// The current state of the edit.
        #[property(get, set = Self::set_state, explicit_notify, builder(EditableAvatarState::default()))]
        state: Cell<EditableAvatarState>,
        /// The state of the avatar edit.
        edit_state: Cell<ActionState>,
        /// Whether the edit button is sensitive.
        edit_sensitive: Cell<bool>,
        /// The state of the avatar removal.
        remove_state: Cell<ActionState>,
        /// Whether the remove button is sensitive.
        remove_sensitive: Cell<bool>,
        /// A temporary paintable to show instead of the avatar.
        #[property(get)]
        temp_paintable: RefCell<Option<gdk::Paintable>>,
        /// The error encountered when loading the temporary avatar, if any.
        temp_error: Cell<Option<ImageError>>,
        temp_paintable_animation_ref: RefCell<Option<CountedRef>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EditableAvatar {
        const NAME: &'static str = "EditableAvatar";
        type Type = super::EditableAvatar;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            klass.set_css_name("editable-avatar");

            klass.install_action_async(
                "editable-avatar.edit-avatar",
                None,
                |obj, _, _| async move {
                    obj.choose_avatar().await;
                },
            );
            klass.install_action("editable-avatar.remove-avatar", None, |obj, _, _| {
                obj.emit_by_name::<()>("remove-avatar", &[]);
            });
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for EditableAvatar {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> = LazyLock::new(|| {
                vec![
                    Signal::builder("edit-avatar")
                        .param_types([gio::File::static_type()])
                        .build(),
                    Signal::builder("remove-avatar").build(),
                ]
            });
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            self.button_remove
                .set_extra_classes(&["destructive-action"]);

            // Watch whether we can remove the avatar.
            let image_present_expr = obj
                .property_expression("data")
                .chain_property::<AvatarData>("image")
                .chain_property::<AvatarImage>("uri-string")
                .chain_closure::<bool>(closure!(|_: Option<glib::Object>, uri: Option<String>| {
                    uri.is_some()
                }));

            let editable_expr = obj.property_expression("editable");
            let remove_not_inhibited_expr =
                expression::not(obj.property_expression("inhibit-remove"));
            let can_remove_expr = expression::and(editable_expr, remove_not_inhibited_expr);

            let button_remove_visible = expression::and(can_remove_expr, image_present_expr);
            button_remove_visible.bind(&*self.button_remove, "visible", glib::Object::NONE);

            // Watch whether the temp avatar is mapped for animations.
            self.temp_avatar.connect_map(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_temp_paintable_state();
                }
            ));
            self.temp_avatar.connect_unmap(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_temp_paintable_state();
                }
            ));
        }
    }

    impl WidgetImpl for EditableAvatar {}
    impl BinImpl for EditableAvatar {}

    impl EditableAvatar {
        /// Set the [`AvatarData`] to display.
        fn set_data(&self, data: Option<AvatarData>) {
            if self.data.obj() == data {
                return;
            }

            self.data.disconnect_signals();

            if let Some(data) = data {
                let image_handler = data.connect_image_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_image();
                    }
                ));

                self.data.set(data, vec![image_handler]);
            }

            self.update_image();
            self.obj().notify_data();
        }

        /// Update the avatar image to watch.
        fn update_image(&self) {
            let image = self.data.obj().and_then(|data| data.image());

            if self.image.obj() == image {
                return;
            }

            self.image.disconnect_signals();

            if let Some(image) = &image {
                let error_handler = image.connect_error_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_error();
                    }
                ));

                self.image.set(image, vec![error_handler]);
            }

            self.update_error();
            self.obj().notify_image();
        }

        /// Set whether this avatar is editable.
        fn set_editable(&self, editable: bool) {
            if self.editable.get() == editable {
                return;
            }

            self.editable.set(editable);
            self.obj().notify_editable();
        }

        /// Set whether to prevent the remove button from showing.
        fn set_inhibit_remove(&self, inhibit: bool) {
            if self.inhibit_remove.get() == inhibit {
                return;
            }

            self.inhibit_remove.set(inhibit);
            self.obj().notify_inhibit_remove();
        }

        /// Set the state of the edit.
        pub(super) fn set_state(&self, state: EditableAvatarState) {
            if self.state.get() == state {
                return;
            }

            match state {
                EditableAvatarState::Default => {
                    self.show_temp_paintable(false);
                    self.set_edit_state(ActionState::Default);
                    self.set_edit_sensitive(true);
                    self.set_remove_state(ActionState::Default);
                    self.set_remove_sensitive(true);

                    self.set_temp_paintable(Ok(None));
                }
                EditableAvatarState::EditInProgress => {
                    self.show_temp_paintable(true);
                    self.set_edit_state(ActionState::Loading);
                    self.set_edit_sensitive(true);
                    self.set_remove_state(ActionState::Default);
                    self.set_remove_sensitive(false);
                }
                EditableAvatarState::EditSuccessful => {
                    self.show_temp_paintable(false);
                    self.set_edit_sensitive(true);
                    self.set_remove_state(ActionState::Default);
                    self.set_remove_sensitive(true);

                    self.set_temp_paintable(Ok(None));

                    // Animation for success.
                    self.set_edit_state(ActionState::Success);
                    glib::timeout_add_local_once(
                        Duration::from_secs(2),
                        clone!(
                            #[weak(rename_to =imp)]
                            self,
                            move || {
                                imp.set_state(EditableAvatarState::Default);
                            }
                        ),
                    );
                }
                EditableAvatarState::RemovalInProgress => {
                    self.show_temp_paintable(true);
                    self.set_edit_state(ActionState::Default);
                    self.set_edit_sensitive(false);
                    self.set_remove_state(ActionState::Loading);
                    self.set_remove_sensitive(true);
                }
            }

            self.state.set(state);
            self.obj().notify_state();
        }

        /// The dimensions of the avatar in this widget.
        fn avatar_dimensions(&self) -> FrameDimensions {
            let scale_factor = self.obj().scale_factor();
            let avatar_size = self.temp_avatar.size();
            let size = (avatar_size * scale_factor)
                .try_into()
                .expect("size and scale factor are positive integers");

            FrameDimensions {
                width: size,
                height: size,
            }
        }

        /// Load the temporary paintable from the given file.
        pub(super) async fn set_temp_paintable_from_file(&self, file: gio::File) {
            let handle = IMAGE_QUEUE.add_file_request(file.into(), Some(self.avatar_dimensions()));
            let paintable = handle.await.map(|image| Some(image.into()));
            self.set_temp_paintable(paintable);
        }

        /// Set the temporary paintable.
        fn set_temp_paintable(&self, paintable: Result<Option<gdk::Paintable>, ImageError>) {
            let (paintable, error) = match paintable {
                Ok(paintable) => (paintable, None),
                Err(error) => (None, Some(error)),
            };

            if *self.temp_paintable.borrow() == paintable {
                return;
            }

            self.temp_paintable.replace(paintable);

            self.update_temp_paintable_state();
            self.set_temp_error(error);
            self.obj().notify_temp_paintable();
        }

        /// Show the temporary paintable instead of the current avatar.
        fn show_temp_paintable(&self, show: bool) {
            let child_name = if show { "temp" } else { "default" };
            self.stack.set_visible_child_name(child_name);
            self.update_error();
        }

        /// Update the state of the temp paintable.
        fn update_temp_paintable_state(&self) {
            self.temp_paintable_animation_ref.take();

            let Some(paintable) = self
                .temp_paintable
                .borrow()
                .clone()
                .and_downcast::<AnimatedImagePaintable>()
            else {
                return;
            };

            if self.temp_avatar.is_mapped() {
                self.temp_paintable_animation_ref
                    .replace(Some(paintable.animation_ref()));
            }
        }

        /// Set the error encountered when loading the temporary avatar, if any.
        fn set_temp_error(&self, error: Option<ImageError>) {
            if self.temp_error.get() == error {
                return;
            }

            self.temp_error.set(error);

            self.update_error();
        }

        /// Update the error that is displayed.
        fn update_error(&self) {
            let error = if self
                .stack
                .visible_child_name()
                .is_some_and(|name| name == "default")
            {
                self.image.obj().and_then(|image| image.error())
            } else {
                self.temp_error.get()
            };

            if let Some(error) = error {
                self.error_img.set_tooltip_text(Some(&error.to_string()));
            }
            self.error_img.set_visible(error.is_some());
        }

        /// The state of the avatar edit.
        pub(super) fn edit_state(&self) -> ActionState {
            self.edit_state.get()
        }

        /// Set the state of the avatar edit.
        fn set_edit_state(&self, state: ActionState) {
            if self.edit_state() == state {
                return;
            }

            self.edit_state.set(state);
        }

        /// Whether the edit button is sensitive.
        fn edit_sensitive(&self) -> bool {
            self.edit_sensitive.get()
        }

        /// Set whether the edit button is sensitive.
        fn set_edit_sensitive(&self, sensitive: bool) {
            if self.edit_sensitive() == sensitive {
                return;
            }

            self.edit_sensitive.set(sensitive);
        }

        /// The state of the avatar removal.
        pub(super) fn remove_state(&self) -> ActionState {
            self.remove_state.get()
        }

        /// Set the state of the avatar removal.
        fn set_remove_state(&self, state: ActionState) {
            if self.remove_state() == state {
                return;
            }

            self.remove_state.set(state);
        }

        /// Whether the remove button is sensitive.
        fn remove_sensitive(&self) -> bool {
            self.remove_sensitive.get()
        }

        /// Set whether the remove button is sensitive.
        fn set_remove_sensitive(&self, sensitive: bool) {
            if self.remove_sensitive() == sensitive {
                return;
            }

            self.remove_sensitive.set(sensitive);
        }
    }
}

glib::wrapper! {
    /// An `Avatar` that can be edited.
    pub struct EditableAvatar(ObjectSubclass<imp::EditableAvatar>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl EditableAvatar {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Reset the state of the avatar.
    pub(crate) fn reset(&self) {
        self.imp().set_state(EditableAvatarState::Default);
    }

    /// Show that an edit is in progress.
    pub(crate) fn edit_in_progress(&self) {
        self.imp().set_state(EditableAvatarState::EditInProgress);
    }

    /// Show that a removal is in progress.
    pub(crate) fn removal_in_progress(&self) {
        self.imp().set_state(EditableAvatarState::RemovalInProgress);
    }

    /// Show that the current ongoing action was successful.
    ///
    /// This is has no effect if no action is ongoing.
    pub(crate) fn success(&self) {
        let imp = self.imp();
        if imp.edit_state() == ActionState::Loading {
            imp.set_state(EditableAvatarState::EditSuccessful);
        } else if imp.remove_state() == ActionState::Loading {
            // The remove button is hidden as soon as the avatar is gone so we
            // don't need a state when it succeeds.
            imp.set_state(EditableAvatarState::Default);
        }
    }

    /// Choose a new avatar.
    pub(super) async fn choose_avatar(&self) {
        let image_filter = gtk::FileFilter::new();
        image_filter.set_name(Some(&gettext("Images")));
        image_filter.add_mime_type("image/*");

        let filters = SingleItemListModel::new(Some(&image_filter));

        let dialog = gtk::FileDialog::builder()
            .title(gettext("Choose Avatar"))
            .modal(true)
            .accept_label(gettext("Choose"))
            .filters(&filters)
            .build();

        let file = match dialog
            .open_future(self.root().and_downcast_ref::<gtk::Window>())
            .await
        {
            Ok(file) => file,
            Err(error) => {
                if error.matches(gtk::DialogError::Dismissed) {
                    debug!("File dialog dismissed by user");
                } else {
                    error!("Could not open avatar file: {error:?}");
                    toast!(self, gettext("Could not open avatar file"));
                }
                return;
            }
        };

        if let Some(content_type) = file
            .query_info_future(
                gio::FILE_ATTRIBUTE_STANDARD_CONTENT_TYPE,
                gio::FileQueryInfoFlags::NONE,
                glib::Priority::LOW,
            )
            .await
            .ok()
            .and_then(|info| info.content_type())
        {
            if gio::content_type_is_a(&content_type, "image/*") {
                self.imp().set_temp_paintable_from_file(file.clone()).await;
                self.emit_by_name::<()>("edit-avatar", &[&file]);
            } else {
                error!("Expected an image, got {content_type}");
                toast!(self, gettext("The chosen file is not an image"));
            }
        } else {
            error!("Could not get the content type of the file");
            toast!(
                self,
                gettext("Could not determine the type of the chosen file")
            );
        }
    }

    /// Connect to the signal emitted when a new avatar is selected.
    pub fn connect_edit_avatar<F: Fn(&Self, gio::File) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "edit-avatar",
            true,
            closure_local!(|obj: Self, file: gio::File| {
                f(&obj, file);
            }),
        )
    }

    /// Connect to the signal emitted when the avatar is removed.
    pub fn connect_remove_avatar<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "remove-avatar",
            true,
            closure_local!(|obj: Self| {
                f(&obj);
            }),
        )
    }
}
