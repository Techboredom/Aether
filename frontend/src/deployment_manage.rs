use common::{DeploymentDetail, MyQuota, UpdateDeploymentRequest};
use leptos::prelude::*;
use leptos::tachys::dom::event_target_value;
use leptos::task::spawn_local;

use crate::result_banner::{ErrorBanner};

use crate::api;
use crate::env_editor::{EnvVars, EnvVarsEditor};
use crate::format::{fixed_request_note, quota_summary};

/// Scale/edit/delete controls for a Deployment, shown in the pod detail panel
/// for any pod that has one (`PodInfo::deployment_name`). The backend is the
/// actual authority on who's allowed to do this — a `user`-role account only
/// ever sees pods it owns to begin with (see `visibility.rs`), so this panel
/// doesn't need to duplicate that check to decide whether to render.
#[component]
pub fn ManageDeploymentSection(deployment_name: String, selected: RwSignal<Option<String>>) -> impl IntoView {
    let loading = RwSignal::new(true);
    let load_error = RwSignal::new(None::<String>);

    let replicas = RwSignal::new(1i32);
    let cpu_request = RwSignal::new(String::new());
    let cpu_limit = RwSignal::new(String::new());
    let memory_request = RwSignal::new(String::new());
    let memory_limit = RwSignal::new(String::new());
    let env_vars = EnvVars::new();
    let generated_secret_key = RwSignal::new(None::<String>);

    let saving = RwSignal::new(false);
    let save_result: RwSignal<Option<Result<String, String>>> = RwSignal::new(None);
    let deleting = RwSignal::new(false);
    let delete_error = RwSignal::new(None::<String>);

    let my_quota: RwSignal<Option<MyQuota>> = RwSignal::new(None);
    let expose_requests = move || my_quota.get().map(|q| q.expose_resource_requests).unwrap_or(true);

    spawn_local(async move {
        if let Ok(quota) = api::get_json::<MyQuota>("/api/quota/me").await {
            my_quota.set(Some(quota));
        }
    });

    {
        let name = deployment_name.clone();
        spawn_local(async move {
            match api::get_json::<DeploymentDetail>(&format!("/api/deployments/{name}")).await {
                    Ok(detail) => {
                        replicas.set(detail.replicas);
                        cpu_request.set(detail.cpu_request.unwrap_or_default());
                        cpu_limit.set(detail.cpu_limit.unwrap_or_default());
                        memory_request.set(detail.memory_request.unwrap_or_default());
                        memory_limit.set(detail.memory_limit.unwrap_or_default());
                        env_vars.set_from(&detail.env);
                        generated_secret_key.set(detail.generated_secret_key);
                    }
                    Err(err) => load_error.set(Some(format!("failed to load deployment: {err}"))),
            }
            loading.set(false);
        });
    }

    view! {
        <section class="manage-deployment">
            <h3>"Manage"</h3>
            {move || {
                if loading.get() {
                    return view! { <p class="empty">"Loading…"</p> }.into_any();
                }
                if let Some(msg) = load_error.get() {
                    return view! { <div class="error">{msg}</div> }.into_any();
                }

                // Rebuilt on every call rather than hoisted above this
                // closure, since this whole block itself is only ever
                // called once loading/load_error settle - defining these
                // outside would make them get moved into the form on the
                // first call and be unavailable on a hypothetical second one.
                let on_submit = {
                    let name = deployment_name.clone();
                    move |ev: web_sys::SubmitEvent| {
                        ev.prevent_default();
                        if saving.get() {
                            return;
                        }
                        let req = UpdateDeploymentRequest {
                            replicas: replicas.get(),
                            cpu_request: if expose_requests() { non_empty(cpu_request.get()) } else { None },
                            cpu_limit: non_empty(cpu_limit.get()),
                            memory_request: if expose_requests() { non_empty(memory_request.get()) } else { None },
                            memory_limit: non_empty(memory_limit.get()),
                            env: env_vars.to_pairs(),
                        };
                        let name = name.clone();
                        saving.set(true);
                        save_result.set(None);
                        spawn_local(async move {
                            let outcome = save(&name, req).await;
                            saving.set(false);
                            save_result.set(Some(outcome));
                        });
                    }
                };

                let on_delete = {
                    let name = deployment_name.clone();
                    move |_| {
                        let confirmed = web_sys::window()
                            .and_then(|w| {
                                w.confirm_with_message(&format!(
                                    "Delete deployment \"{name}\"? This removes it and its Service, if any. This can't be undone."
                                ))
                                .ok()
                            })
                            .unwrap_or(false);
                        if !confirmed || deleting.get() {
                            return;
                        }
                        let name = name.clone();
                        deleting.set(true);
                        delete_error.set(None);
                        spawn_local(async move {
                            let outcome = api::delete(&format!("/api/deployments/{name}")).await;
                            deleting.set(false);
                            match outcome {
                                Ok(()) => selected.set(None),
                                Err(err) => delete_error.set(Some(format!("failed to delete: {err}"))),
                            }
                        });
                    }
                };

                view! {
                    <div>
                        <form class="deploy-form" on:submit=on_submit>
                            <label>
                                "Replicas"
                                <input
                                    type="number"
                                    min="0"
                                    step="1"
                                    prop:value=move || replicas.get().to_string()
                                    on:input=move |ev| {
                                        if let Ok(v) = event_target_value(&ev).parse() {
                                            replicas.set(v);
                                        }
                                    }
                                />
                            </label>

                            {move || {
                                my_quota.get().map(|q| view! { <p class="hint">{quota_summary(&q)}</p> })
                            }}
                            {move || {
                                my_quota.get().and_then(|q| fixed_request_note(&q)).map(|note| {
                                    view! { <p class="hint">{note}</p> }
                                })
                            }}

                            <fieldset>
                                <legend>"CPU"</legend>
                                <Show when=expose_requests>
                                    <label>
                                        "Request"
                                        <input
                                            type="text"
                                            placeholder="e.g. 250m"
                                            prop:value=move || cpu_request.get()
                                            on:input=move |ev| cpu_request.set(event_target_value(&ev))
                                        />
                                    </label>
                                </Show>
                                <label>
                                    "Limit"
                                    <input
                                        type="text"
                                        placeholder="e.g. 1"
                                        prop:value=move || cpu_limit.get()
                                        on:input=move |ev| cpu_limit.set(event_target_value(&ev))
                                    />
                                </label>
                            </fieldset>

                            <fieldset>
                                <legend>"Memory"</legend>
                                <Show when=expose_requests>
                                    <label>
                                        "Request"
                                        <input
                                            type="text"
                                            placeholder="e.g. 256Mi"
                                            prop:value=move || memory_request.get()
                                            on:input=move |ev| memory_request.set(event_target_value(&ev))
                                        />
                                    </label>
                                </Show>
                                <label>
                                    "Limit"
                                    <input
                                        type="text"
                                        placeholder="e.g. 512Mi"
                                        prop:value=move || memory_limit.get()
                                        on:input=move |ev| memory_limit.set(event_target_value(&ev))
                                    />
                                </label>
                            </fieldset>

                            <fieldset class="env-fieldset">
                                <legend>"Environment variables"</legend>
                                {move || {
                                    generated_secret_key
                                        .get()
                                        .map(|key| {
                                            view! {
                                                <p class="hint">
                                                    {format!(
                                                        "\"{key}\" is an auto-generated credential and isn't shown or editable here.",
                                                    )}
                                                </p>
                                            }
                                        })
                                }}
                                <EnvVarsEditor vars=env_vars />
                            </fieldset>

                            <div class="form-actions">
                                <button type="submit" disabled=move || saving.get()>
                                    {move || if saving.get() { "Saving…" } else { "Save changes" }}
                                </button>
                            </div>
                        </form>

                        {move || {
                            save_result.get().map(|res| match res {
                                Ok(msg) => view! { <div class="success">{msg}</div> }.into_any(),
                                Err(msg) => view! { <div class="error">{msg}</div> }.into_any(),
                            })
                        }}

                        <div class="form-actions">
                            <button type="button" class="danger-button" disabled=move || deleting.get() on:click=on_delete>
                                {move || if deleting.get() { "Deleting…" } else { "Delete deployment" }}
                            </button>
                        </div>
                        <ErrorBanner error=delete_error />
                    </div>
                }
                    .into_any()
            }}
        </section>
    }
}

fn non_empty(s: String) -> Option<String> {
    let s = s.trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

async fn save(name: &str, req: UpdateDeploymentRequest) -> Result<String, String> {
    let _: DeploymentDetail =
        api::put_json(&format!("/api/deployments/{name}"), &req).await.map_err(|err| format!("Failed to save: {err}"))?;
    Ok("Saved.".to_string())
}
