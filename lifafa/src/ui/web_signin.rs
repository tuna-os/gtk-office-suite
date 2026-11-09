//! A provider's sign-in page in a window of Hylki's own (#329), for an OAuth
//! client registered with a redirect Hylki cannot listen on, such as
//! Microsoft's `…/oauth2/nativeclient` that Evolution's client uses. The
//! page runs in a private session with nothing kept afterwards, and the
//! redirect is caught as the page navigates to it, so it never loads.
//!
//! The usual sign-in, with a loopback redirect, stays in the browser
//! (`oauth::run_flow`).

use adw::prelude::*;
use webkit6::prelude::*;

use crate::config::OAuthSettings;
use crate::i18n::i18n;
use crate::services::oauth::Authorization;

/// Open the sign-in for `settings` and hand `done` the request and the
/// authorization code once the provider redirects, or why there is none
/// (the window was closed, or the provider refused). `user` is the address
/// being signed in, when known: it picks the identity broker's account, and
/// with `hint` Microsoft's sign-in goes straight to it.
pub(crate) fn run(
    parent: Option<&gtk::Window>,
    settings: &OAuthSettings,
    user: Option<String>,
    hint: bool,
    done: impl FnOnce(Result<(Authorization, String), String>) + 'static,
) {
    let extra = if settings.token_url.contains("microsoftonline") {
        crate::services::oauth::microsoft_prompt(user.as_deref().filter(|_| hint))
    } else {
        String::new()
    };
    let request = match Authorization::new(settings, settings.redirect_uri.trim(), &extra) {
        Ok(r) => r,
        Err(e) => return done(Err(e)),
    };
    let window = adw::Window::builder()
        .title(i18n("Sign In"))
        .default_width(520)
        .default_height(680)
        .modal(true)
        .build();
    window.set_transient_for(parent);
    let header = adw::HeaderBar::new();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);

    let session = webkit6::NetworkSession::new_ephemeral();
    let view = webkit6::WebView::builder()
        .network_session(&session)
        .build();
    view.set_vexpand(true);
    toolbar.set_content(Some(&view));
    window.set_content(Some(&toolbar));

    // `done` runs once: on the redirect, or when the window closes first.
    let done: DoneSlot = std::rc::Rc::new(std::cell::RefCell::new(Some(Box::new(done))));
    let request = std::rc::Rc::new(std::cell::RefCell::new(Some(request)));
    {
        let done = done.clone();
        let request = request.clone();
        let settings = settings.clone();
        let window = window.downgrade();
        view.connect_decide_policy(move |_, decision, kind| {
            if kind != webkit6::PolicyDecisionType::NavigationAction {
                return false;
            }
            let Some(nav) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>() else {
                return false;
            };
            let uri = nav
                .navigation_action()
                .and_then(|mut a| a.request())
                .and_then(|r| r.uri())
                .map(|u| u.to_string())
                .unwrap_or_default();
            let answer = request
                .borrow()
                .as_ref()
                .and_then(|r| r.answer(&uri, &settings));
            let Some(answer) = answer else { return false };
            decision.ignore();
            if let (Some(cb), Some(req)) = (done.borrow_mut().take(), request.borrow_mut().take()) {
                cb(answer.map(|code| (req, code)));
            }
            if let Some(w) = window.upgrade() {
                w.close();
            }
            true
        });
    }
    {
        let done = done.clone();
        window.connect_close_request(move |_| {
            if let Some(cb) = done.borrow_mut().take() {
                cb(Err(i18n("The sign-in window was closed.")));
            }
            gtk::glib::Propagation::Proceed
        });
    }
    window.present();
    let Some(url) = request.borrow().as_ref().map(|r| r.url.clone()) else {
        return;
    };
    // Behind Conditional Access that wants a managed device, the identity
    // broker's device cookie goes in first (#329, see `ms_broker`), as
    // Evolution's sign-in window does it. Asking the broker can take a
    // moment, so it is asked off the main thread; without one the page
    // loads as it is.
    let settings = settings.clone();
    gtk::glib::spawn_future_local(async move {
        let cookie = gtk::gio::spawn_blocking(move || {
            crate::services::ms_broker::sso_cookie(&settings, user.as_deref())
        })
        .await
        .ok()
        .flatten();
        if let (Some(c), Some(jar)) = (cookie, session.cookie_manager()) {
            let mut cookie =
                webkit6::soup::Cookie::new(&c.name, &c.value, "login.microsoftonline.com", "/", -1);
            cookie.set_secure(true);
            cookie.set_http_only(true);
            if let Err(e) = jar.add_cookie_future(&cookie).await {
                tracing::warn!(target: "hylki::oauth", "identity broker: the cookie was not taken: {e}");
            }
        }
        view.load_uri(&url);
    });
}

/// The sign-in's answer, taken once: by the redirect, or by the window
/// closing first.
type DoneSlot = std::rc::Rc<
    std::cell::RefCell<Option<Box<dyn FnOnce(Result<(Authorization, String), String>)>>>,
>;
