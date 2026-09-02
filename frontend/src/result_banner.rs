use leptos::prelude::*;

/// The success/error line every form in the app shows under itself.
///
/// `None` renders nothing, `Ok` a success line, `Err` an error line — which
/// is exactly the `Option<Result<String, String>>` each form already keeps
/// for its last submission.
#[component]
pub fn ResultBanner(result: RwSignal<Option<Result<String, String>>>) -> impl IntoView {
    view! {
        {move || {
            result
                .get()
                .map(|res| match res {
                    Ok(message) => view! { <div class="success">{message}</div> }.into_any(),
                    Err(message) => view! { <div class="error">{message}</div> }.into_any(),
                })
        }}
    }
}

/// The error-only variant, for the places that surface a failed *load*
/// rather than a failed submission.
#[component]
pub fn ErrorBanner(error: RwSignal<Option<String>>) -> impl IntoView {
    view! { {move || error.get().map(|message| view! { <div class="error">{message}</div> })} }
}
