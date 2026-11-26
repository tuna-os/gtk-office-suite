//! Moderation operations (kick, ban, unban) on a [`Room`].

use matrix_sdk::{Result as MatrixResult, room::Room as MatrixRoom};
use ruma::{OwnedUserId, UserId};
use tracing::error;

use super::Room;
use crate::spawn_tokio;

/// A moderation action applied to a list of users.
#[derive(Clone, Copy)]
enum Action {
    Kick,
    Ban,
    Unban,
}

impl Action {
    fn name(self) -> &'static str {
        match self {
            Self::Kick => "kick",
            Self::Ban => "ban",
            Self::Unban => "unban",
        }
    }

    async fn apply(
        self,
        room: &MatrixRoom,
        user_id: &UserId,
        reason: Option<&str>,
    ) -> MatrixResult<()> {
        match self {
            Self::Kick => room.kick_user(user_id, reason).await,
            Self::Ban => room.ban_user(user_id, reason).await,
            Self::Unban => room.unban_user(user_id, reason).await,
        }
    }
}

impl Room {
    /// Apply the given action to the given `(user_id, reason)` tuples.
    ///
    /// Returns `Ok(())` if all the requests succeed, otherwise the list of
    /// users for whom it failed.
    async fn moderate<'a>(
        &self,
        action: Action,
        users: &'a [(OwnedUserId, Option<String>)],
    ) -> Result<(), Vec<&'a UserId>> {
        let users_clone = users.to_owned();
        let matrix_room = self.matrix_room().clone();
        let handle = spawn_tokio!(async move {
            let futures = users_clone
                .iter()
                .map(|(user_id, reason)| action.apply(&matrix_room, user_id, reason.as_deref()));
            futures_util::future::join_all(futures).await
        });

        let mut failed = Vec::new();
        for (index, result) in handle
            .await
            .expect("task was not aborted")
            .iter()
            .enumerate()
        {
            if let Err(error) = result {
                error!(
                    "Could not {} user with ID {}: {error}",
                    action.name(),
                    users[index].0
                );
                failed.push(&*users[index].0);
            }
        }

        if failed.is_empty() {
            Ok(())
        } else {
            Err(failed)
        }
    }

    /// Kick the given users from this room.
    ///
    /// The users are a list of `(user_id, reason)` tuples.
    ///
    /// Returns `Ok(())` if all the kicks are sent successfully, otherwise
    /// returns the list of users who could not be kicked.
    pub(crate) async fn kick<'a>(
        &self,
        users: &'a [(OwnedUserId, Option<String>)],
    ) -> Result<(), Vec<&'a UserId>> {
        self.moderate(Action::Kick, users).await
    }

    /// Ban the given users from this room.
    ///
    /// The users are a list of `(user_id, reason)` tuples.
    ///
    /// Returns `Ok(())` if all the bans are sent successfully, otherwise
    /// returns the list of users who could not be banned.
    pub(crate) async fn ban<'a>(
        &self,
        users: &'a [(OwnedUserId, Option<String>)],
    ) -> Result<(), Vec<&'a UserId>> {
        self.moderate(Action::Ban, users).await
    }

    /// Unban the given users from this room.
    ///
    /// The users are a list of `(user_id, reason)` tuples.
    ///
    /// Returns `Ok(())` if all the unbans are sent successfully, otherwise
    /// returns the list of users who could not be unbanned.
    pub(crate) async fn unban<'a>(
        &self,
        users: &'a [(OwnedUserId, Option<String>)],
    ) -> Result<(), Vec<&'a UserId>> {
        self.moderate(Action::Unban, users).await
    }
}
