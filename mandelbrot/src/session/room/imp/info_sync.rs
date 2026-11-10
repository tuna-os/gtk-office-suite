use futures_util::StreamExt;
use gtk::{
    glib::{self, SendWeakRef},
    subclass::prelude::*,
};
use matrix_sdk::RoomInfo;

use super::Room;
use crate::{spawn, spawn_tokio};

impl Room {
    /// Watch the SDK's room info for changes to the room state.
    pub(super) fn watch_room_info(&self) {
        let matrix_room = self.matrix_room();
        let subscriber = matrix_room.subscribe_info();

        let obj_weak = SendWeakRef::from(self.obj().downgrade());
        let fut = subscriber.for_each(move |room_info| {
            let obj_weak = obj_weak.clone();
            async move {
                let ctx = glib::MainContext::default();
                ctx.spawn(async move {
                    spawn!(async move {
                        if let Some(obj) = obj_weak.upgrade() {
                            obj.imp().update_with_room_info(room_info).await;
                        }
                    });
                });
            }
        });
        spawn_tokio!(fut);
    }

    /// Update this room with the given SDK room info.
    pub(super) async fn update_with_room_info(&self, room_info: RoomInfo) {
        self.aliases.update();
        self.update_name();
        self.update_display_name().await;
        self.update_avatar();
        self.update_topic();
        self.update_category().await;
        self.update_is_direct().await;
        self.update_is_marked_unread().await;
        self.update_tombstone();
        self.set_joined_members_count(room_info.joined_members_count());
        self.update_is_encrypted().await;
        self.join_rule.update(room_info.join_rule());
        self.update_guests_allowed();
        self.update_history_visibility();
    }
}
