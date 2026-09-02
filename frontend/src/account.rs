use common::ChangePasswordRequest;
use leptos::prelude::*;
use leptos::tachys::dom::event_target_value;
use leptos::task::spawn_local;

use crate::api;

#[component]
pub fn ChangePasswordPanel(open: RwSignal<bool>) -> impl IntoView {
    let current_password = RwSignal::new(String::new());
    let new_password = RwSignal::new(String::new());
    let confirm_password = RwSignal::new(String::new());
    let saving = RwSignal::new(false);
    let result: RwSignal<Option<Result<String, String>>> = RwSignal::new(None);

    let close = move |_| open.set(false);

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        if saving.get() {
            return;
        }
        if new_password.get() != confirm_password.get() {
            result.set(Some(Err("new password and confirmation don't match".to_string())));
            return;
        }
        let req = ChangePasswordRequest { current_password: current_password.get(), new_password: new_password.get() };
        saving.set(true);
        result.set(None);
        spawn_local(async move {
            let outcome = submit(req).await;
            saving.set(false);
            if outcome.is_ok() {
                current_password.set(String::new());
                new_password.set(String::new());
                confirm_password.set(String::new());
            }
            result.set(Some(outcome));
        });
    };

    view! {
        <div class="overlay" on:click=close>
            <div class="panel" on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()>
                <div class="panel-head">
                    <h2>"Change password"</h2>
                    <button class="icon-button" on:click=close>
                        "✕"
                    </button>
                </div>

                <form class="deploy-form" on:submit=on_submit>
                    <label>
                        "Current password"
                        <input
                            type="password"
                            required=true
                            autocomplete="current-password"
                            prop:value=move || current_password.get()
                            on:input=move |ev| current_password.set(event_target_value(&ev))
                        />
                    </label>
                    <label>
                        "New password"
                        <input
                            type="password"
                            required=true
                            minlength="8"
                            autocomplete="new-password"
                            prop:value=move || new_password.get()
                            on:input=move |ev| new_password.set(event_target_value(&ev))
                        />
                    </label>
                    <label>
                        "Confirm new password"
                        <input
                            type="password"
                            required=true
                            minlength="8"
                            autocomplete="new-password"
                            prop:value=move || confirm_password.get()
                            on:input=move |ev| confirm_password.set(event_target_value(&ev))
                        />
                    </label>
                    <button type="submit" disabled=move || saving.get()>
                        {move || if saving.get() { "Saving…" } else { "Change password" }}
                    </button>
                </form>

                {move || {
                    result
                        .get()
                        .map(|res| match res {
                            Ok(msg) => view! { <div class="success">{msg}</div> }.into_any(),
                            Err(msg) => view! { <div class="error">{msg}</div> }.into_any(),
                        })
                }}
            </div>
        </div>
    }
}

async fn submit(req: ChangePasswordRequest) -> Result<String, String> {
    api::put_empty("/api/me/password", &req).await.map_err(|err| format!("Failed to change password: {err}"))?;
    Ok("Password changed. Your other sessions have been logged out.".to_string())
}
