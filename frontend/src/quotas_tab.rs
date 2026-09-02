use common::{QuotaLimits, QuotaSettings, UserQuotaEntry};
use leptos::prelude::*;
use leptos::tachys::dom::{event_target_checked, event_target_value};
use leptos::task::spawn_local;

use crate::api;
use crate::result_banner::{ErrorBanner, ResultBanner};

use crate::format::{bytes, millicores};

#[component]
pub fn QuotasTab() -> impl IntoView {
    let settings_loading = RwSignal::new(true);
    let settings_error = RwSignal::new(None::<String>);
    let global_cpu_limit = RwSignal::new(String::new());
    let global_memory_limit = RwSignal::new(String::new());
    let global_gpu_limit = RwSignal::new(String::new());
    let expose_resource_requests = RwSignal::new(true);
    let fixed_cpu_request = RwSignal::new(String::new());
    let fixed_memory_request = RwSignal::new(String::new());
    let settings_saving = RwSignal::new(false);
    let settings_result: RwSignal<Option<Result<String, String>>> = RwSignal::new(None);

    let users: RwSignal<Vec<UserQuotaEntry>> = RwSignal::new(Vec::new());
    let list_error = RwSignal::new(None::<String>);

    let edit_target: RwSignal<Option<(i32, String)>> = RwSignal::new(None);
    let edit_cpu_limit = RwSignal::new(String::new());
    let edit_memory_limit = RwSignal::new(String::new());
    let edit_gpu_limit = RwSignal::new(String::new());
    let edit_saving = RwSignal::new(false);
    let edit_result: RwSignal<Option<Result<String, String>>> = RwSignal::new(None);

    let refresh_settings = move || {
        spawn_local(async move {
            match api::get_json::<QuotaSettings>("/api/quota/settings").await {
                Ok(s) => {
                    settings_error.set(None);
                    global_cpu_limit.set(s.limits.cpu_limit.unwrap_or_default());
                    global_memory_limit.set(s.limits.memory_limit.unwrap_or_default());
                    global_gpu_limit.set(s.limits.gpu_limit.map(|v| v.to_string()).unwrap_or_default());
                    expose_resource_requests.set(s.expose_resource_requests);
                    fixed_cpu_request.set(s.fixed_cpu_request.unwrap_or_default());
                    fixed_memory_request.set(s.fixed_memory_request.unwrap_or_default());
                }
                Err(err) => settings_error.set(Some(format!("failed to load quota settings: {err}"))),
            }
            settings_loading.set(false);
        });
    };
    refresh_settings();

    let refresh_users = move || {
        spawn_local(async move {
            match api::get_json::<Vec<UserQuotaEntry>>("/api/quota/users").await {
                Ok(list) => {
                    list_error.set(None);
                    users.set(list);
                }
                Err(err) => list_error.set(Some(format!("failed to load user quotas: {err}"))),
            }
        });
    };
    refresh_users();

    let on_settings_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        if settings_saving.get() {
            return;
        }
        let req = QuotaSettings {
            limits: QuotaLimits {
                cpu_limit: non_empty(global_cpu_limit.get()),
                memory_limit: non_empty(global_memory_limit.get()),
                gpu_limit: non_empty(global_gpu_limit.get()).and_then(|v| v.parse().ok()),
            },
            expose_resource_requests: expose_resource_requests.get(),
            fixed_cpu_request: non_empty(fixed_cpu_request.get()),
            fixed_memory_request: non_empty(fixed_memory_request.get()),
        };
        settings_saving.set(true);
        settings_result.set(None);
        spawn_local(async move {
            let outcome = save_settings(req).await;
            settings_saving.set(false);
            settings_result.set(Some(outcome));
        });
    };

    let clear_override = move |id: i32| {
        spawn_local(async move {
            match api::delete(&format!("/api/quota/users/{id}")).await {
                Ok(()) => refresh_users(),
                Err(err) => list_error.set(Some(format!("failed to clear override: {err}"))),
            }
        });
    };

    let on_edit_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        if edit_saving.get() {
            return;
        }
        let Some((id, _)) = edit_target.get() else { return };
        let req = QuotaLimits {
            cpu_limit: non_empty(edit_cpu_limit.get()),
            memory_limit: non_empty(edit_memory_limit.get()),
            gpu_limit: non_empty(edit_gpu_limit.get()).and_then(|v| v.parse().ok()),
        };
        edit_saving.set(true);
        edit_result.set(None);
        spawn_local(async move {
            let outcome = save_override(id, req).await;
            edit_saving.set(false);
            match outcome {
                Ok(msg) => {
                    edit_result.set(Some(Ok(msg)));
                    edit_target.set(None);
                    refresh_users();
                }
                Err(err) => edit_result.set(Some(Err(err))),
            }
        });
    };

    view! {
        <div class="tab-panel">
            <h3 class="section-heading">"Global default quota"</h3>
            <p class="hint">
                "Applies to any user with no override below. Checked against resource "
                <strong>"limits"</strong>
                ", not requests. Leave a field blank for unlimited."
            </p>
            <ErrorBanner error=settings_error />
            <Show when=move || !settings_loading.get()>
                <form class="deploy-form" on:submit=on_settings_submit>
                    <label>
                        "CPU limit (cores, optional)"
                        <input
                            type="text"
                            placeholder="e.g. 4 — blank means unlimited"
                            prop:value=move || global_cpu_limit.get()
                            on:input=move |ev| global_cpu_limit.set(event_target_value(&ev))
                        />
                    </label>
                    <label>
                        "Memory limit (optional)"
                        <input
                            type="text"
                            placeholder="e.g. 16Gi — blank means unlimited"
                            prop:value=move || global_memory_limit.get()
                            on:input=move |ev| global_memory_limit.set(event_target_value(&ev))
                        />
                    </label>
                    <label>
                        "GPU limit (optional)"
                        <input
                            type="number"
                            min="0"
                            step="1"
                            placeholder="blank means unlimited"
                            prop:value=move || global_gpu_limit.get()
                            on:input=move |ev| global_gpu_limit.set(event_target_value(&ev))
                        />
                    </label>
                    <label class="checkbox">
                        <input
                            type="checkbox"
                            prop:checked=move || expose_resource_requests.get()
                            on:change=move |ev| expose_resource_requests.set(event_target_checked(&ev))
                        />
                        "Show separate CPU/memory \"request\" fields on the Launch tab and the Pods tab's manage panel (unchecking hides them and applies the fixed requests below to every launch/edit instead)"
                    </label>
                    <Show when=move || !expose_resource_requests.get()>
                        <label>
                            "Fixed CPU request (optional)"
                            <input
                                type="text"
                                placeholder="e.g. 100m — blank leaves the request unset (Kubernetes then defaults it to match the limit)"
                                prop:value=move || fixed_cpu_request.get()
                                on:input=move |ev| fixed_cpu_request.set(event_target_value(&ev))
                            />
                        </label>
                        <label>
                            "Fixed memory request (optional)"
                            <input
                                type="text"
                                placeholder="e.g. 128Mi — blank leaves the request unset"
                                prop:value=move || fixed_memory_request.get()
                                on:input=move |ev| fixed_memory_request.set(event_target_value(&ev))
                            />
                        </label>
                    </Show>
                    <div class="form-actions">
                        <button type="submit" disabled=move || settings_saving.get()>
                            {move || if settings_saving.get() { "Saving…" } else { "Save global quota" }}
                        </button>
                    </div>
                </form>
            </Show>
            <ResultBanner result=settings_result />

            <h3 class="section-heading">"Per-user overrides"</h3>
            <ErrorBanner error=list_error />
            <div class="table-wrap">
                <table>
                    <thead>
                        <tr>
                            <th>"Username"</th>
                            <th>"CPU used / limit"</th>
                            <th>"Memory used / limit"</th>
                            <th>"GPUs used / limit"</th>
                            <th></th>
                        </tr>
                    </thead>
                    <tbody>
                        <For each=move || users.get() key=|u| u.user_id let(u)>
                            {
                                let id = u.user_id;
                                let username = u.username.clone();
                                let has_override = u.quota_override.is_some();
                                let cpu_limit_label =
                                    u.quota_override.as_ref().and_then(|o| o.cpu_limit.clone()).unwrap_or_else(|| "global default".to_string());
                                let memory_limit_label =
                                    u.quota_override.as_ref().and_then(|o| o.memory_limit.clone()).unwrap_or_else(|| "global default".to_string());
                                let gpu_limit_label = u
                                    .quota_override
                                    .as_ref()
                                    .and_then(|o| o.gpu_limit)
                                    .map(|v| v.to_string())
                                    .unwrap_or_else(|| "global default".to_string());
                                let edit_username = username.clone();
                                let quota_override = u.quota_override.clone();
                                view! {
                                    <tr>
                                        <td>{username}</td>
                                        <td>{format!("{} / {}", millicores(Some(u.used_cpu_millicores)), cpu_limit_label)}</td>
                                        <td>{format!("{} / {}", bytes(Some(u.used_memory_bytes)), memory_limit_label)}</td>
                                        <td>{format!("{} / {}", u.used_gpu_count, gpu_limit_label)}</td>
                                        <td class="table-actions">
                                            <button
                                                type="button"
                                                class="icon-button"
                                                on:click=move |_| {
                                                    edit_target.set(Some((id, edit_username.clone())));
                                                    let o = quota_override.clone().unwrap_or_default();
                                                    edit_cpu_limit.set(o.cpu_limit.unwrap_or_default());
                                                    edit_memory_limit.set(o.memory_limit.unwrap_or_default());
                                                    edit_gpu_limit.set(o.gpu_limit.map(|v| v.to_string()).unwrap_or_default());
                                                    edit_result.set(None);
                                                }
                                            >
                                                "Edit override"
                                            </button>
                                            <Show when=move || has_override>
                                                <button type="button" class="icon-button" on:click=move |_| clear_override(id)>
                                                    "Clear override"
                                                </button>
                                            </Show>
                                        </td>
                                    </tr>
                                }
                            }
                        </For>
                    </tbody>
                </table>
                <Show when=move || users.get().is_empty() && list_error.get().is_none()>
                    <p class="empty">"No users yet."</p>
                </Show>
            </div>

            {move || {
                edit_target
                    .get()
                    .map(|(_, target_username)| {
                        view! {
                            <h3 class="section-heading">{format!("Quota override for \"{target_username}\"")}</h3>
                            <form class="deploy-form" on:submit=on_edit_submit>
                                <label>
                                    "CPU limit (cores, optional)"
                                    <input
                                        type="text"
                                        placeholder="blank means unlimited"
                                        prop:value=move || edit_cpu_limit.get()
                                        on:input=move |ev| edit_cpu_limit.set(event_target_value(&ev))
                                    />
                                </label>
                                <label>
                                    "Memory limit (optional)"
                                    <input
                                        type="text"
                                        placeholder="blank means unlimited"
                                        prop:value=move || edit_memory_limit.get()
                                        on:input=move |ev| edit_memory_limit.set(event_target_value(&ev))
                                    />
                                </label>
                                <label>
                                    "GPU limit (optional)"
                                    <input
                                        type="number"
                                        min="0"
                                        step="1"
                                        placeholder="blank means unlimited"
                                        prop:value=move || edit_gpu_limit.get()
                                        on:input=move |ev| edit_gpu_limit.set(event_target_value(&ev))
                                    />
                                </label>
                                <div class="form-actions">
                                    <button type="submit" disabled=move || edit_saving.get()>
                                        {move || if edit_saving.get() { "Saving…" } else { "Save override" }}
                                    </button>
                                    <button type="button" class="secondary-button" on:click=move |_| edit_target.set(None)>
                                        "Cancel"
                                    </button>
                                </div>
                            </form>
                        }
                    })
            }}
            <ResultBanner result=edit_result />
        </div>
    }
}

fn non_empty(s: String) -> Option<String> {
    let s = s.trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

async fn save_settings(req: QuotaSettings) -> Result<String, String> {
    let _: QuotaSettings = api::put_json("/api/quota/settings", &req).await.map_err(|err| format!("Failed to save: {err}"))?;
    Ok("Saved global quota.".to_string())
}

async fn save_override(id: i32, req: QuotaLimits) -> Result<String, String> {
    let _: QuotaLimits =
        api::put_json(&format!("/api/quota/users/{id}"), &req).await.map_err(|err| format!("Failed to save: {err}"))?;
    Ok("Saved override.".to_string())
}
