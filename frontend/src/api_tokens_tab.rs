use common::{ApiTokenCreated, ApiTokenEntry, CreateApiTokenRequest};
use leptos::prelude::*;
use leptos::tachys::dom::event_target_value;
use leptos::task::spawn_local;

use crate::api;
use crate::result_banner::{ErrorBanner, ResultBanner};

/// Lets an admin mint/list/revoke API tokens for their own account — an
/// alternate credential to a session cookie, meant for scripts/automation
/// (e.g. `curl -H "Authorization: Bearer <token>"`) rather than a browser.
/// See README's "Admin API tokens" section.
#[component]
pub fn ApiTokensTab() -> impl IntoView {
    let tokens: RwSignal<Vec<ApiTokenEntry>> = RwSignal::new(Vec::new());
    let list_error = RwSignal::new(None::<String>);

    let name = RwSignal::new(String::new());
    let saving = RwSignal::new(false);
    let form_result: RwSignal<Option<Result<String, String>>> = RwSignal::new(None);

    let refresh = move || {
        spawn_local(async move {
            match api::get_json::<Vec<ApiTokenEntry>>("/api/tokens").await {
                Ok(list) => {
                    list_error.set(None);
                    tokens.set(list);
                }
                Err(err) => list_error.set(Some(format!("failed to load tokens: {err}"))),
            }
        });
    };
    refresh();

    let revoke = move |id: i32| {
        let confirmed = web_sys::window()
            .and_then(|w| {
                w.confirm_with_message("Revoke this token? Anything still using it will immediately stop working.").ok()
            })
            .unwrap_or(false);
        if !confirmed {
            return;
        }
        spawn_local(async move {
            match api::delete(&format!("/api/tokens/{id}")).await {
                Ok(()) => refresh(),
                Err(err) => list_error.set(Some(format!("failed to revoke token: {err}"))),
            }
        });
    };

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        if saving.get() {
            return;
        }
        let req = CreateApiTokenRequest { name: name.get().trim().to_string() };
        saving.set(true);
        form_result.set(None);
        spawn_local(async move {
            let outcome = create(req).await;
            saving.set(false);
            match outcome {
                Ok(msg) => {
                    form_result.set(Some(Ok(msg)));
                    name.set(String::new());
                    refresh();
                }
                Err(err) => form_result.set(Some(Err(err))),
            }
        });
    };

    view! {
        <div class="tab-panel">
            <ErrorBanner error=list_error />

            <div class="table-wrap">
                <table>
                    <thead>
                        <tr>
                            <th>"Name"</th>
                            <th>"Created"</th>
                            <th>"Last used"</th>
                            <th></th>
                        </tr>
                    </thead>
                    <tbody>
                        <For each=move || tokens.get() key=|t| t.id let(t)>
                            {
                                let id = t.id;
                                let last_used = t.last_used_at.clone().unwrap_or_else(|| "never".to_string());
                                view! {
                                    <tr>
                                        <td>{t.name.clone()}</td>
                                        <td>{t.created_at.clone()}</td>
                                        <td>{last_used}</td>
                                        <td class="table-actions">
                                            <button type="button" class="icon-button" on:click=move |_| revoke(id)>
                                                "Revoke"
                                            </button>
                                        </td>
                                    </tr>
                                }
                            }
                        </For>
                    </tbody>
                </table>
                <Show when=move || tokens.get().is_empty() && list_error.get().is_none()>
                    <p class="empty">"No API tokens yet."</p>
                </Show>
            </div>

            <h3 class="section-heading">"New token"</h3>
            <form class="deploy-form" on:submit=on_submit>
                <label>
                    "Name"
                    <input
                        type="text"
                        required=true
                        maxlength="100"
                        placeholder="e.g. CI automation"
                        prop:value=move || name.get()
                        on:input=move |ev| name.set(event_target_value(&ev))
                    />
                </label>
                <p class="hint">
                    "Authenticates as your own account (with your own role) via \"Authorization: Bearer <token>\" — no session cookie needed. The value is shown once, right after creation, and never again; if you lose it, revoke it and create a new one."
                </p>
                <button type="submit" disabled=move || saving.get()>
                    {move || if saving.get() { "Creating…" } else { "Create token" }}
                </button>
            </form>

            <ResultBanner result=form_result />
        </div>
    }
}

async fn create(req: CreateApiTokenRequest) -> Result<String, String> {
    let created: ApiTokenCreated =
        api::post_json("/api/tokens", &req).await.map_err(|err| format!("Failed to create token: {err}"))?;
    Ok(format!(
        "Created \"{}\". Copy this value now — it won't be shown again: {}",
        created.name, created.token
    ))
}
