use common::{CreateDeploymentRequest, CreateDeploymentResponse, ImageEntry};
use gloo_net::http::Request;
use leptos::prelude::*;
use leptos::tachys::dom::event_target_value;
use leptos::task::spawn_local;

use crate::templates::{Template, CUSTOM_TEMPLATE_ID, TEMPLATES};

const ENV_ROWS: usize = 3;

#[component]
pub fn CreateDeploymentTab() -> impl IntoView {
    let images: RwSignal<Vec<ImageEntry>> = RwSignal::new(Vec::new());
    let images_error = RwSignal::new(None::<String>);

    let selected_template = RwSignal::new(CUSTOM_TEMPLATE_ID);

    let name = RwSignal::new(String::new());
    let image = RwSignal::new(String::new());
    let replicas = RwSignal::new("1".to_string());
    let container_port = RwSignal::new(String::new());
    let cpu_request = RwSignal::new(String::new());
    let cpu_limit = RwSignal::new(String::new());
    let memory_request = RwSignal::new(String::new());
    let memory_limit = RwSignal::new(String::new());
    let accelerator_type = RwSignal::new(String::new());
    let accelerator_count = RwSignal::new(String::new());
    let env_keys: [RwSignal<String>; ENV_ROWS] = std::array::from_fn(|_| RwSignal::new(String::new()));
    let env_values: [RwSignal<String>; ENV_ROWS] = std::array::from_fn(|_| RwSignal::new(String::new()));
    let args_text = RwSignal::new(String::new());
    let notes = RwSignal::new(None::<&'static str>);

    let submitting = RwSignal::new(false);
    let result: RwSignal<Option<Result<String, String>>> = RwSignal::new(None);

    spawn_local(async move {
        match Request::get("/api/images").send().await {
            Ok(resp) if resp.ok() => match resp.json::<Vec<ImageEntry>>().await {
                Ok(list) => images.set(list),
                Err(err) => images_error.set(Some(format!("failed to parse image list: {err}"))),
            },
            Ok(resp) => images_error.set(Some(format!("failed to load images: HTTP {}", resp.status()))),
            Err(err) => images_error.set(Some(format!("failed to load images: {err}"))),
        }
    });

    let apply_template = move |t: &'static Template| {
        selected_template.set(t.id);
        notes.set(Some(t.notes));
        name.set(t.id.to_string());
        image.set(t.image.to_string());
        container_port.set(t.port.map(|p| p.to_string()).unwrap_or_default());
        cpu_request.set(t.cpu_request.to_string());
        cpu_limit.set(t.cpu_limit.to_string());
        memory_request.set(t.memory_request.to_string());
        memory_limit.set(t.memory_limit.to_string());
        accelerator_type.set(t.accelerator_type.to_string());
        accelerator_count.set(if t.accelerator_count > 0 { t.accelerator_count.to_string() } else { String::new() });
        for (i, key_signal) in env_keys.iter().enumerate() {
            key_signal.set(t.env_keys.get(i).map(|s| s.to_string()).unwrap_or_default());
        }
        for value_signal in env_values.iter() {
            value_signal.set(String::new());
        }
        args_text.set(t.args.join("\n"));
    };

    let select_custom = move |_| {
        selected_template.set(CUSTOM_TEMPLATE_ID);
        notes.set(None);
        name.set(String::new());
        image.set(String::new());
        container_port.set(String::new());
        cpu_request.set(String::new());
        cpu_limit.set(String::new());
        memory_request.set(String::new());
        memory_limit.set(String::new());
        accelerator_type.set(String::new());
        accelerator_count.set(String::new());
        for key_signal in env_keys.iter() {
            key_signal.set(String::new());
        }
        for value_signal in env_values.iter() {
            value_signal.set(String::new());
        }
        args_text.set(String::new());
    };

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        if submitting.get() {
            return;
        }

        let accel_type = accelerator_type.get();
        let accel_count = if accel_type.is_empty() {
            None
        } else {
            accelerator_count.get().trim().parse::<i64>().ok()
        };
        let env: Vec<(String, String)> =
            env_keys.iter().zip(env_values.iter()).map(|(k, v)| (k.get().trim().to_string(), v.get())).collect();
        let args: Vec<String> = args_text.get().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();

        let req = CreateDeploymentRequest {
            name: name.get().trim().to_string(),
            image: image.get(),
            replicas: replicas.get().trim().parse().unwrap_or(1),
            cpu_request: non_empty(cpu_request.get()),
            cpu_limit: non_empty(cpu_limit.get()),
            memory_request: non_empty(memory_request.get()),
            memory_limit: non_empty(memory_limit.get()),
            accelerator_type: if accel_type.is_empty() { None } else { Some(accel_type) },
            accelerator_count: accel_count,
            container_port: container_port.get().trim().parse().ok(),
            env,
            args,
        };

        submitting.set(true);
        result.set(None);
        spawn_local(async move {
            let outcome = submit(req).await;
            submitting.set(false);
            result.set(Some(outcome));
        });
    };

    view! {
        <div class="tab-panel">
            {move || images_error.get().map(|msg| view! { <div class="error">{msg}</div> })}

            <div class="template-picker">
                <button
                    type="button"
                    class="tab-button"
                    class:active=move || selected_template.get() == CUSTOM_TEMPLATE_ID
                    on:click=select_custom
                >
                    "Custom"
                </button>
                {TEMPLATES
                    .iter()
                    .map(|t| {
                        let id = t.id;
                        view! {
                            <button
                                type="button"
                                class="tab-button"
                                class:active=move || selected_template.get() == id
                                on:click=move |_| apply_template(t)
                            >
                                {t.label}
                            </button>
                        }
                    })
                    .collect::<Vec<_>>()}
            </div>

            {move || notes.get().map(|n| view! { <div class="template-notes">{n}</div> })}

            <form class="deploy-form" on:submit=on_submit>
                <label>
                    "Name"
                    <input
                        type="text"
                        required=true
                        prop:value=move || name.get()
                        on:input=move |ev| name.set(event_target_value(&ev))
                    />
                </label>

                <label>
                    "Image"
                    <input
                        type="text"
                        list="image-catalog"
                        required=true
                        placeholder="e.g. nginx:stable"
                        prop:value=move || image.get()
                        on:input=move |ev| image.set(event_target_value(&ev))
                    />
                    <datalist id="image-catalog">
                        <For each=move || images.get() key=|img| img.id let(img)>
                            <option value=img.image.clone()>{img.name.clone()}</option>
                        </For>
                    </datalist>
                </label>

                <label>
                    "Replicas"
                    <input
                        type="number"
                        min="0"
                        step="1"
                        prop:value=move || replicas.get()
                        on:input=move |ev| replicas.set(event_target_value(&ev))
                    />
                </label>

                <label>
                    "Container port (optional)"
                    <input
                        type="number"
                        min="1"
                        step="1"
                        placeholder="e.g. 8080 — creates a LoadBalancer Service"
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
                    <legend>"Environment variables (optional)"</legend>
                    {(0..ENV_ROWS)
                        .map(|i| {
                            let key_signal = env_keys[i];
                            let value_signal = env_values[i];
                            view! {
                                <div class="env-row">
                                    <label>
                                        "Key"
                                        <input
                                            type="text"
                                            placeholder="e.g. PASSWORD"
                                            prop:value=move || key_signal.get()
                                            on:input=move |ev| key_signal.set(event_target_value(&ev))
                                        />
                                    </label>
                                    <label>
                                        "Value"
                                        <input
                                            type="text"
                                            prop:value=move || value_signal.get()
                                            on:input=move |ev| value_signal.set(event_target_value(&ev))
                                        />
                                    </label>
                                </div>
                            }
                        })
                        .collect::<Vec<_>>()}
                </fieldset>

                <label>
                    "Command arguments (optional, one per line)"
                    <textarea
                        rows="2"
                        placeholder="--model=..."
                        prop:value=move || args_text.get()
                        on:input=move |ev| args_text.set(event_target_value(&ev))
                    ></textarea>
                </label>

                <button type="submit" disabled=move || submitting.get()>
                    {move || if submitting.get() { "Launching…" } else { "Launch" }}
                </button>
            </form>

            {move || {
                result.get().map(|res| match res {
                    Ok(msg) => view! { <div class="success">{msg}</div> }.into_any(),
                    Err(msg) => view! { <div class="error">{msg}</div> }.into_any(),
                })
            }}
        </div>
    }
}

fn non_empty(s: String) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

async fn submit(req: CreateDeploymentRequest) -> Result<String, String> {
    let resp = Request::post("/api/deployments")
        .json(&req)
        .map_err(|err| format!("failed to encode request: {err}"))?
        .send()
        .await
        .map_err(|err| format!("request failed: {err}"))?;

    if resp.ok() {
        let created: CreateDeploymentResponse =
            resp.json().await.map_err(|err| format!("failed to parse response: {err}"))?;
        let mut msg = format!("Created deployment \"{}\" in namespace \"{}\".", created.name, created.namespace);
        if let (Some(service), Some(port)) = (created.service_name, created.container_port) {
            msg.push_str(&format!(
                " Exposed via Service \"{service}\" on port {port} — check `kubectl get svc -n {}` for its external IP.",
                created.namespace
            ));
        }
        Ok(msg)
    } else {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let message = body.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error");
        Err(format!("Failed to create deployment: {message}"))
    }
}
