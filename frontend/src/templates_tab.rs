use common::{SaveTemplateRequest, TemplateEntry};
use gloo_net::http::Request;
use leptos::prelude::*;
use leptos::tachys::dom::{event_target_checked, event_target_value};
use leptos::task::spawn_local;

use crate::env_editor::{EnvVars, EnvVarsEditor};
use crate::format;

#[component]
pub fn TemplatesTab() -> impl IntoView {
    let templates: RwSignal<Vec<TemplateEntry>> = RwSignal::new(Vec::new());
    let list_error = RwSignal::new(None::<String>);

    let editing_id = RwSignal::new(None::<i32>);
    let name = RwSignal::new(String::new());
    let image = RwSignal::new(String::new());
    let container_port = RwSignal::new(String::new());
    let cpu_request = RwSignal::new(String::new());
    let cpu_limit = RwSignal::new(String::new());
    let memory_request = RwSignal::new(String::new());
    let memory_limit = RwSignal::new(String::new());
    let accelerator_type = RwSignal::new(String::new());
    let accelerator_count = RwSignal::new(String::new());
    let env_vars = EnvVars::new();
    let args_text = RwSignal::new(String::new());
    let notes_text = RwSignal::new(String::new());
    let secret_env_key = RwSignal::new(String::new());
    let proxy_enabled = RwSignal::new(false);
    let strip_prefix = RwSignal::new(false);
    let public_service = RwSignal::new(true);

    let saving = RwSignal::new(false);
    let form_result: RwSignal<Option<Result<String, String>>> = RwSignal::new(None);

    let refresh = move || {
        spawn_local(async move {
            match Request::get("/api/templates").send().await {
                Ok(resp) if resp.ok() => match resp.json::<Vec<TemplateEntry>>().await {
                    Ok(list) => {
                        list_error.set(None);
                        templates.set(list);
                    }
                    Err(err) => list_error.set(Some(format!("failed to parse template list: {err}"))),
                },
                Ok(resp) => list_error.set(Some(format!("failed to load templates: HTTP {}", resp.status()))),
                Err(err) => list_error.set(Some(format!("failed to load templates: {err}"))),
            }
        });
    };
    refresh();

    let clear_form = move || {
        editing_id.set(None);
        name.set(String::new());
        image.set(String::new());
        container_port.set(String::new());
        cpu_request.set(String::new());
        cpu_limit.set(String::new());
        memory_request.set(String::new());
        memory_limit.set(String::new());
        accelerator_type.set(String::new());
        accelerator_count.set(String::new());
        env_vars.set_from(&[]);
        args_text.set(String::new());
        notes_text.set(String::new());
        secret_env_key.set(String::new());
        proxy_enabled.set(false);
        strip_prefix.set(false);
        public_service.set(true);
    };

    let load_into_form = move |t: &TemplateEntry| {
        editing_id.set(Some(t.id));
        name.set(t.name.clone());
        image.set(t.image.clone());
        container_port.set(t.container_port.map(|p| p.to_string()).unwrap_or_default());
        cpu_request.set(t.cpu_request.clone());
        cpu_limit.set(t.cpu_limit.clone());
        memory_request.set(t.memory_request.clone());
        memory_limit.set(t.memory_limit.clone());
        accelerator_type.set(t.accelerator_type.clone());
        accelerator_count.set(t.accelerator_count.map(|c| c.to_string()).unwrap_or_default());
        env_vars.set_from(&t.env);
        args_text.set(t.args.join("\n"));
        notes_text.set(t.notes.clone());
        secret_env_key.set(t.secret_env_key.clone().unwrap_or_default());
        proxy_enabled.set(t.proxy_enabled);
        strip_prefix.set(t.strip_prefix);
        public_service.set(t.public_service);
        form_result.set(None);
    };

    let delete_template = move |id: i32| {
        let confirmed = web_sys::window()
            .and_then(|w| w.confirm_with_message("Delete this template? This can't be undone.").ok())
            .unwrap_or(false);
        if !confirmed {
            return;
        }
        spawn_local(async move {
            let outcome = Request::delete(&format!("/api/templates/{id}")).send().await;
            match outcome {
                Ok(resp) if resp.ok() => {
                    if editing_id.get() == Some(id) {
                        clear_form();
                    }
                    refresh();
                }
                Ok(resp) => list_error.set(Some(format!("failed to delete template: HTTP {}", resp.status()))),
                Err(err) => list_error.set(Some(format!("failed to delete template: {err}"))),
            }
        });
    };

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        if saving.get() {
            return;
        }

        let accel_type = accelerator_type.get();
        let accel_count = if accel_type.is_empty() { None } else { accelerator_count.get().trim().parse::<i64>().ok() };
        let args: Vec<String> = args_text.get().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();

        let req = SaveTemplateRequest {
            name: name.get().trim().to_string(),
            image: image.get().trim().to_string(),
            container_port: container_port.get().trim().parse().ok(),
            cpu_request: cpu_request.get().trim().to_string(),
            cpu_limit: cpu_limit.get().trim().to_string(),
            memory_request: memory_request.get().trim().to_string(),
            memory_limit: memory_limit.get().trim().to_string(),
            accelerator_type: accel_type,
            accelerator_count: accel_count,
            env: env_vars.to_pairs(),
            args,
            notes: notes_text.get(),
            secret_env_key: {
                let key = secret_env_key.get().trim().to_string();
                if key.is_empty() { None } else { Some(key) }
            },
            proxy_enabled: proxy_enabled.get(),
            strip_prefix: strip_prefix.get(),
            public_service: public_service.get(),
        };

        let id = editing_id.get();
        saving.set(true);
        form_result.set(None);
        spawn_local(async move {
            let outcome = save(id, req).await;
            saving.set(false);
            match outcome {
                Ok(msg) => {
                    form_result.set(Some(Ok(msg)));
                    clear_form();
                    refresh();
                }
                Err(err) => form_result.set(Some(Err(err))),
            }
        });
    };

    view! {
        <div class="tab-panel">
            {move || list_error.get().map(|msg| view! { <div class="error">{msg}</div> })}

            <div class="table-wrap">
                <table>
                    <thead>
                        <tr>
                            <th>"Name"</th>
                            <th>"Image"</th>
                            <th>"Port"</th>
                            <th>"CPU"</th>
                            <th>"Memory"</th>
                            <th>"Accelerator"</th>
                            <th>"Secret"</th>
                            <th>"Proxy"</th>
                            <th>"Public"</th>
                            <th></th>
                        </tr>
                    </thead>
                    <tbody>
                        <For each=move || templates.get() key=|t| t.id let(t)>
                            {
                                let t_for_edit = t.clone();
                                let id = t.id;
                                view! {
                                    <tr>
                                        <td>{t.name.clone()}</td>
                                        <td>{t.image.clone()}</td>
                                        <td>{t.container_port.map(|p| p.to_string()).unwrap_or_else(|| "—".into())}</td>
                                        <td>{format!("{} / {}", or_dash(&t.cpu_request), or_dash(&t.cpu_limit))}</td>
                                        <td>{format!("{} / {}", or_dash(&t.memory_request), or_dash(&t.memory_limit))}</td>
                                        <td>{format::accelerator_summary(&t.accelerator_type, t.accelerator_count)}</td>
                                        <td>{t.secret_env_key.clone().unwrap_or_else(|| "—".into())}</td>
                                        <td>{if t.proxy_enabled { "yes" } else { "—" }}</td>
                                        <td>{if t.public_service { "yes" } else { "no" }}</td>
                                        <td class="table-actions">
                                            <button type="button" class="icon-button" on:click=move |_| load_into_form(&t_for_edit)>
                                                "Edit"
                                            </button>
                                            <button type="button" class="icon-button" on:click=move |_| delete_template(id)>
                                                "Delete"
                                            </button>
                                        </td>
                                    </tr>
                                }
                            }
                        </For>
                    </tbody>
                </table>
                <Show when=move || templates.get().is_empty() && list_error.get().is_none()>
                    <p class="empty">"No templates yet — add one below."</p>
                </Show>
            </div>

            <h3 class="section-heading">
                {move || if editing_id.get().is_some() { "Edit template" } else { "New template" }}
            </h3>

            <form class="deploy-form" on:submit=on_submit>
                <label>
                    "Name"
                    <input
                        type="text"
                        required=true
                        maxlength="100"
                        prop:value=move || name.get()
                        on:input=move |ev| name.set(event_target_value(&ev))
                    />
                </label>

                <label>
                    "Image"
                    <input
                        type="text"
                        required=true
                        maxlength="512"
                        placeholder="e.g. ollama/ollama:latest"
                        prop:value=move || image.get()
                        on:input=move |ev| image.set(event_target_value(&ev))
                    />
                </label>

                <label>
                    "Container port (optional)"
                    <input
                        type="number"
                        min="1"
                        max="65535"
                        step="1"
                        prop:value=move || container_port.get()
                        on:input=move |ev| container_port.set(event_target_value(&ev))
                    />
                </label>

                <fieldset>
                    <legend>"CPU"</legend>
                    <label>
                        "Request"
                        <input
                            type="text"
                            placeholder="e.g. 250m"
                            prop:value=move || cpu_request.get()
                            on:input=move |ev| cpu_request.set(event_target_value(&ev))
                        />
                    </label>
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
                    <label>
                        "Request"
                        <input
                            type="text"
                            placeholder="e.g. 256Mi"
                            prop:value=move || memory_request.get()
                            on:input=move |ev| memory_request.set(event_target_value(&ev))
                        />
                    </label>
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

                <fieldset>
                    <legend>"Accelerator (optional)"</legend>
                    <label>
                        "Type"
                        <select
                            prop:value=move || accelerator_type.get()
                            on:change=move |ev| accelerator_type.set(event_target_value(&ev))
                        >
                            <option value="">"None"</option>
                            <option value="nvidia.com/gpu">"nvidia.com/gpu"</option>
                            <option value="amd.com/gpu">"amd.com/gpu"</option>
                            <option value="gpu.intel.com/i915">"gpu.intel.com/i915"</option>
                        </select>
                    </label>
                    <label>
                        "Count"
                        <input
                            type="number"
                            min="1"
                            step="1"
                            disabled=move || accelerator_type.get().is_empty()
                            prop:value=move || accelerator_count.get()
                            on:input=move |ev| accelerator_count.set(event_target_value(&ev))
                        />
                    </label>
                </fieldset>

                <fieldset class="env-fieldset">
                    <legend>"Environment variable defaults (optional)"</legend>
                    <EnvVarsEditor vars=env_vars />
                </fieldset>

                <label>
                    "Command arguments (optional, one per line)"
                    <textarea
                        rows="2"
                        placeholder="--model=<huggingface-model-id>"
                        prop:value=move || args_text.get()
                        on:input=move |ev| args_text.set(event_target_value(&ev))
                    ></textarea>
                </label>

                <label>
                    "Notes (shown when this template is selected on the Launch tab)"
                    <textarea
                        rows="2"
                        maxlength="2000"
                        prop:value=move || notes_text.get()
                        on:input=move |ev| notes_text.set(event_target_value(&ev))
                    ></textarea>
                </label>

                <label>
                    "Auto-generate a secret for this env var (optional)"
                    <input
                        type="text"
                        maxlength="128"
                        placeholder="e.g. JUPYTER_TOKEN, PASSWORD, VLLM_API_KEY"
                        prop:value=move || secret_env_key.get()
                        on:input=move |ev| secret_env_key.set(event_target_value(&ev))
                    />
                </label>

                <label class="checkbox">
                    <input
                        type="checkbox"
                        prop:checked=move || proxy_enabled.get()
                        on:change=move |ev| proxy_enabled.set(event_target_checked(&ev))
                    />
                    "Also reachable via Aether's own /proxy/<name>/ route (injects the generated secret above, if any)"
                </label>

                <label class="checkbox">
                    <input
                        type="checkbox"
                        disabled=move || !proxy_enabled.get()
                        prop:checked=move || strip_prefix.get()
                        on:change=move |ev| strip_prefix.set(event_target_checked(&ev))
                    />
                    "Strip the /proxy/<name>/ prefix before forwarding (for apps like RStudio that expect to run at the root path, unlike JupyterLab)"
                </label>

                <label class="checkbox">
                    <input
                        type="checkbox"
                        prop:checked=move || public_service.get()
                        on:change=move |ev| public_service.set(event_target_checked(&ev))
                    />
                    "Public LoadBalancer Service (uncheck to make it internal — reachable only from inside the cluster, e.g. by other tooling pods, or via Aether's proxy for apps with no auth of their own)"
                </label>

                <div class="form-actions">
                    <button type="submit" disabled=move || saving.get()>
                        {move || {
                            if saving.get() {
                                "Saving…"
                            } else if editing_id.get().is_some() {
                                "Save changes"
                            } else {
                                "Create template"
                            }
                        }}
                    </button>
                    <Show when=move || editing_id.get().is_some()>
                        <button type="button" class="secondary-button" on:click=move |_| clear_form()>
                            "Cancel"
                        </button>
                    </Show>
                </div>
            </form>

            {move || {
                form_result.get().map(|res| match res {
                    Ok(msg) => view! { <div class="success">{msg}</div> }.into_any(),
                    Err(msg) => view! { <div class="error">{msg}</div> }.into_any(),
                })
            }}
        </div>
    }
}

fn or_dash(s: &str) -> &str {
    if s.trim().is_empty() { "—" } else { s }
}

async fn save(id: Option<i32>, req: SaveTemplateRequest) -> Result<String, String> {
    let builder = match id {
        Some(id) => Request::put(&format!("/api/templates/{id}")),
        None => Request::post("/api/templates"),
    };
    let resp = builder
        .json(&req)
        .map_err(|err| format!("failed to encode request: {err}"))?
        .send()
        .await
        .map_err(|err| format!("request failed: {err}"))?;

    if resp.ok() {
        let saved: common::TemplateEntry =
            resp.json().await.map_err(|err| format!("failed to parse response: {err}"))?;
        Ok(format!("Saved template \"{}\".", saved.name))
    } else {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let message = body.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error");
        Err(format!("Failed to save template: {message}"))
    }
}
