use gtk::{
    gdk, glib,
    glib::{clone, closure_local},
    prelude::*,
    subclass::prelude::*,
};
use ruma::{
    OwnedMxcUri, api::client::media::get_content_thumbnail::v3::Method,
    events::room::avatar::ImageInfo,
};

use crate::{
    session::Session,
    spawn,
    utils::{
        CountedRef,
        media::{
            FrameDimensions,
            image::{
                ImageError, ImageRequestPriority, ImageSource, ThumbnailDownloader,
                ThumbnailSettings,
            },
        },
    },
};

/// The source of an avatar's URI.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, glib::Enum)]
#[enum_type(name = "AvatarUriSource")]
pub enum AvatarUriSource {
    /// The URI comes from a Matrix user.
    #[default]
    User,
    /// The URI comes from a Matrix room.
    Room,
}

/// The size of the paintable to load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AvatarPaintableSize {
    /// A small paintable, of size [`AvatarImage::SMALL_PAINTABLE_SIZE`].
    Small,
    /// A big paintable, of size [`AvatarImage::BIG_PAINTABLE_SIZE`].
    Big,
}

impl AvatarPaintableSize {
    /// The size in pixels for this paintable size.
    fn size(self) -> u32 {
        match self {
            Self::Small => AvatarImage::SMALL_PAINTABLE_SIZE,
            Self::Big => AvatarImage::BIG_PAINTABLE_SIZE,
        }
    }
}

impl From<i32> for AvatarPaintableSize {
    fn from(value: i32) -> Self {
        let value = u32::try_from(value).unwrap_or_default();
        if value <= AvatarImage::SMALL_PAINTABLE_SIZE {
            Self::Small
        } else {
            Self::Big
        }
    }
}

mod imp {
    use std::{
        cell::{Cell, OnceCell, RefCell},
        marker::PhantomData,
        sync::LazyLock,
    };

    use glib::subclass::Signal;

    use super::*;

    #[derive(Debug, glib::Properties)]
    #[properties(wrapper_type = super::AvatarImage)]
    pub struct AvatarImage {
        /// The current session.
        #[property(get, construct_only)]
        session: OnceCell<Session>,
        /// The Matrix URI of the avatar.
        uri: RefCell<Option<OwnedMxcUri>>,
        /// The Matrix URI of the `AvatarImage`, as a string.
        #[property(get = Self::uri_string)]
        uri_string: PhantomData<Option<String>>,
        /// Information about the avatar.
        info: RefCell<Option<ImageInfo>>,
        /// The source of the URI avatar.
        #[property(get, construct_only, builder(AvatarUriSource::default()))]
        uri_source: Cell<AvatarUriSource>,
        /// The scale factor to use to load the cached paintable.
        #[property(get, set = Self::set_scale_factor, explicit_notify, default = 1, minimum = 1)]
        scale_factor: Cell<u32>,
        /// The counted reference for the small paintable.
        ///
        /// The small paintable is cached indefinitely after the first reference
        /// is taken.
        small_paintable_ref: OnceCell<CountedRef>,
        /// The cached paintable of the avatar at small size, if any.
        #[property(get)]
        small_paintable: RefCell<Option<gdk::Paintable>>,
        /// The counted reference for the big paintable.
        ///
        /// The big paintable is cached after the first reference is taken and
        /// dropped when the last reference is dropped.
        big_paintable_ref: OnceCell<CountedRef>,
        /// The cached paintable of the avatar at big size, if any.
        #[property(get)]
        big_paintable: RefCell<Option<gdk::Paintable>>,
        /// The last error encountered when loading the cached paintable of the
        /// avatar, if any.
        pub(super) error: Cell<Option<ImageError>>,
    }

    impl Default for AvatarImage {
        fn default() -> Self {
            Self {
                session: Default::default(),
                uri: Default::default(),
                uri_string: Default::default(),
                info: Default::default(),
                uri_source: Default::default(),
                scale_factor: Cell::new(1),
                small_paintable_ref: Default::default(),
                small_paintable: Default::default(),
                big_paintable_ref: Default::default(),
                big_paintable: Default::default(),
                error: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AvatarImage {
        const NAME: &'static str = "AvatarImage";
        type Type = super::AvatarImage;
    }

    #[glib::derived_properties]
    impl ObjectImpl for AvatarImage {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("error-changed").build()]);
            SIGNALS.as_ref()
        }
    }

    impl AvatarImage {
        /// The Matrix URI of the `AvatarImage`.
        pub(super) fn uri(&self) -> Option<OwnedMxcUri> {
            self.uri.borrow().clone()
        }

        /// Set the Matrix URI of the `AvatarImage`.
        ///
        /// Returns whether the URI changed.
        pub(super) fn set_uri(&self, uri: Option<OwnedMxcUri>) {
            if *self.uri.borrow() == uri {
                return;
            }

            let has_uri = uri.is_some();
            self.uri.replace(uri);
            self.obj().notify_uri_string();

            if has_uri && self.small_paintable_ref().count() != 0 {
                spawn!(
                    glib::Priority::LOW,
                    clone!(
                        #[weak(rename_to = imp)]
                        self,
                        async move {
                            imp.load_small_paintable(false).await;
                        }
                    )
                );
            } else {
                // Reset the paintable so it is reloaded later.
                self.small_paintable.take();
                self.error.take();
            }

            if has_uri && self.big_paintable_ref().count() != 0 {
                spawn!(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    async move {
                        imp.load_big_paintable().await;
                    }
                ));
            } else {
                // Reset the error so the paintable can be reloaded later.
                self.error.take();
            }
        }

        /// The Matrix URI of the `AvatarImage`, as a string.
        fn uri_string(&self) -> Option<String> {
            self.uri.borrow().as_ref().map(ToString::to_string)
        }

        /// Information about the avatar.
        pub(super) fn info(&self) -> Option<ImageInfo> {
            self.info.borrow().clone()
        }

        /// Set information about the avatar.
        pub(super) fn set_info(&self, info: Option<ImageInfo>) {
            self.info.replace(info);
        }

        /// Set the scale factor to use to load the cached paintable.
        ///
        /// Only the biggest size will be stored.
        fn set_scale_factor(&self, scale_factor: u32) {
            if self.scale_factor.get() >= scale_factor {
                return;
            }

            self.scale_factor.set(scale_factor);
            self.obj().notify_scale_factor();

            if self.small_paintable_ref().count() != 0 {
                spawn!(
                    glib::Priority::LOW,
                    clone!(
                        #[weak(rename_to = imp)]
                        self,
                        async move {
                            imp.load_small_paintable(false).await;
                        }
                    )
                );
            } else {
                // Reset the paintable so it is reloaded later.
                self.small_paintable.take();
                self.error.take();
            }

            if self.big_paintable_ref().count() != 0 {
                spawn!(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    async move {
                        imp.load_big_paintable().await;
                    }
                ));
            } else {
                // Reset the error so the paintable can be reloaded later.
                self.error.take();
            }
        }

        /// The counted reference for the small paintable.
        pub(super) fn small_paintable_ref(&self) -> &CountedRef {
            self.small_paintable_ref.get_or_init(|| {
                CountedRef::new(
                    || {},
                    clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move || {
                            if imp.small_paintable.borrow().is_none() && imp.error.get().is_none() {
                                spawn!(
                                    glib::Priority::LOW,
                                    clone!(
                                        #[weak]
                                        imp,
                                        async move {
                                            imp.load_small_paintable(false).await;
                                        }
                                    )
                                );
                            }
                        }
                    ),
                )
            })
        }

        /// Load the small paintable.
        pub(super) async fn load_small_paintable(&self, high_priority: bool) {
            let priority = if high_priority {
                ImageRequestPriority::High
            } else {
                ImageRequestPriority::Low
            };
            let paintable = self.load(AvatarPaintableSize::Small, priority).await;

            if self.small_paintable_ref().count() == 0 {
                // The last reference was dropped while we were loading the paintable, do not
                // cache it.
                return;
            }

            let (paintable, error) = match paintable {
                Ok(paintable) => (paintable, None),
                Err(error) => (None, Some(error)),
            };

            if *self.small_paintable.borrow() != paintable {
                self.small_paintable.replace(paintable);
                self.obj().notify_small_paintable();
            }

            self.set_error(error);
        }

        /// The counted reference for the big paintable.
        pub(super) fn big_paintable_ref(&self) -> &CountedRef {
            self.big_paintable_ref.get_or_init(|| {
                CountedRef::new(
                    clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move || {
                            imp.big_paintable.take();
                        }
                    ),
                    clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move || {
                            if imp.big_paintable.borrow().is_none() && imp.error.get().is_none() {
                                spawn!(clone!(
                                    #[weak]
                                    imp,
                                    async move {
                                        imp.load_big_paintable().await;
                                    }
                                ));
                            }
                        }
                    ),
                )
            })
        }

        /// Load the big paintable.
        async fn load_big_paintable(&self) {
            let paintable = self
                .load(AvatarPaintableSize::Big, ImageRequestPriority::High)
                .await;

            if self.big_paintable_ref().count() == 0 {
                // The last reference was dropped while we were loading the paintable, do not
                // cache it.
                return;
            }

            let (paintable, error) = match paintable {
                Ok(paintable) => (paintable, None),
                Err(error) => (None, Some(error)),
            };

            if *self.big_paintable.borrow() != paintable {
                self.big_paintable.replace(paintable);
                self.obj().notify_big_paintable();
            }

            self.set_error(error);
        }

        /// Set the error encountered when loading the avatar, if any.
        fn set_error(&self, error: Option<ImageError>) {
            if self.error.get() == error {
                return;
            }

            self.error.set(error);
            self.obj().emit_by_name::<()>("error-changed", &[]);
        }

        /// Load a paintable of the avatar for the given size.
        async fn load(
            &self,
            size: AvatarPaintableSize,
            priority: ImageRequestPriority,
        ) -> Result<Option<gdk::Paintable>, ImageError> {
            let Some(uri) = self.uri() else {
                // We do not have an avatar to load.
                return Ok(None);
            };

            let client = self.session.get().expect("session is initialized").client();
            let info = self.info();

            let dimension = size.size();
            let scale_factor = self.scale_factor.get();
            let dimensions = FrameDimensions {
                width: dimension,
                height: dimension,
            }
            .scale(scale_factor);

            let downloader = ThumbnailDownloader {
                main: ImageSource {
                    source: (&uri).into(),
                    info: info.as_ref().map(Into::into),
                },
                // Avatars are not encrypted so we should always generate the thumbnail from the
                // original.
                alt: None,
            };
            let settings = ThumbnailSettings {
                dimensions,
                method: Method::Crop,
                animated: true,
                prefer_thumbnail: true,
            };

            downloader
                .download(client, settings, priority)
                .await
                .map(|image| Some(image.into()))
        }
    }
}

glib::wrapper! {
    /// The image data for an avatar.
    pub struct AvatarImage(ObjectSubclass<imp::AvatarImage>);
}

impl AvatarImage {
    /// The small size of the paintable.
    ///
    /// This is usually the size presented in the timeline or the sidebar. This
    /// is also the size of the avatar in GNOME Shell notifications.
    ///
    /// This matches an avatar of size `48` or smaller. This size is cached
    /// indefinitely after the first [`AvatarImage::small_paintable_ref()`] is
    /// taken.
    pub(crate) const SMALL_PAINTABLE_SIZE: u32 = 48;

    /// The big size of the paintable.
    ///
    /// This is usually the size presented in the room details or user profile.
    ///
    /// This matches an avatar of size `150` or smaller. This is only cached
    /// when at least one [`AvatarImage::big_paintable_ref()`] is held.
    pub(crate) const BIG_PAINTABLE_SIZE: u32 = 150;

    /// Construct a new `AvatarImage` with the given session, Matrix URI and
    /// avatar info.
    pub(crate) fn new(
        session: &Session,
        uri_source: AvatarUriSource,
        uri: Option<OwnedMxcUri>,
        info: Option<ImageInfo>,
    ) -> Self {
        let obj = glib::Object::builder::<Self>()
            .property("session", session)
            .property("uri-source", uri_source)
            .build();

        obj.set_uri_and_info(uri, info);
        obj
    }

    /// Set the Matrix URI and information of the avatar.
    pub(crate) fn set_uri_and_info(&self, uri: Option<OwnedMxcUri>, info: Option<ImageInfo>) {
        let imp = self.imp();
        imp.set_info(info);
        imp.set_uri(uri);
    }

    /// The Matrix URI of the avatar.
    pub(crate) fn uri(&self) -> Option<OwnedMxcUri> {
        self.imp().uri()
    }

    /// Get a small paintable ref.
    pub(crate) fn small_paintable_ref(&self) -> CountedRef {
        self.imp().small_paintable_ref().clone()
    }

    /// Get a big paintable ref.
    pub(crate) fn big_paintable_ref(&self) -> CountedRef {
        self.imp().big_paintable_ref().clone()
    }

    /// Get the small paintable.
    ///
    /// We first try to get it from the cache, and load it if it is not cached.
    pub(crate) async fn load_small_paintable(&self) -> Result<Option<gdk::Paintable>, ImageError> {
        if let Some(paintable) = self.small_paintable() {
            return Ok(Some(paintable));
        }

        if let Some(error) = self.error() {
            return Err(error);
        }

        self.imp().load_small_paintable(true).await;

        if let Some(paintable) = self.small_paintable() {
            return Ok(Some(paintable));
        }

        if let Some(error) = self.error() {
            return Err(error);
        }

        Ok(None)
    }

    /// The last error encountered when loading the paintable of the avatar, if
    /// any.
    pub(crate) fn error(&self) -> Option<ImageError> {
        self.imp().error.get()
    }

    /// Connect to the signal emitted when the error changed.
    pub fn connect_error_changed<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "error-changed",
            true,
            closure_local!(|obj: Self| {
                f(&obj);
            }),
        )
    }
}
