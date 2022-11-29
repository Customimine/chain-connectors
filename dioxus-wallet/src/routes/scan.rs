use crate::components::alerts::{Alert, ALERTS};
use crate::qrcode::scan_qrcode;
use dioxus::prelude::*;
use dioxus_router::use_router;
use fermi::*;

#[allow(non_snake_case)]
#[inline_props]
pub fn Scan(cx: Scope) -> Element {
    #[cfg(target_os = "ios")]
    dioxus_desktop::use_window(&cx).pop_view();
    let alerts = use_atom_ref(&cx, ALERTS);
    let router = use_router(&cx);
    let fut = use_future(&cx, (), move |_| scan_qrcode(&cx));
    let alert = match fut.value() {
        Some(Ok(url)) => Some(Alert::info(url.to_string())),
        Some(Err(error)) => Some(Alert::error(error.to_string())),
        None => None,
    };
    if let Some(alert) = alert {
        alerts.write().push(alert);
        router.pop_route();
    }
    None
}
