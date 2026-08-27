use common::{CreateDeploymentRequest, CreateDeploymentResponse, ImageEntry};
use gloo_net::http::Request;
use leptos::prelude::*;
use leptos::tachys::dom::event_target_value;
use leptos::task::spawn_local;

#[component]
pub fn CreateDeploymentTab() -> impl IntoView {
    let images: RwSignal<Vec<ImageEntry>> = RwSignal::new(Vec::new());
    let images_error = RwSignal::new(None::<String>);

    let name = RwSignal::new(String::new());
    let image = RwSignal::new(String::new());
    let replicas = RwSignal::new("1".to_string());
    let cpu_request = RwSignal::new(String::new());
    let cpu_limit = RwSignal::new(String::new());
    let memory_request = RwSignal::new(String::new());
    let memory_limit = RwSignal::new(String::new());
    let accelerator_type = RwSignal::new(String::new());
    let accelerator_count = RwSignal::new(String::new());

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
                    <select
                        required=true
                        prop:value=move || image.get()
                        on:change=move |ev| image.set(event_target_value(&ev))
                    >
                        <option value="" disabled=true>
                            "Select an image…"
                        </option>
                        <For each=move || images.get() key=|img| img.id let(img)>
                            <option value=img.image.clone()>{format!("{} — {}", img.name, img.image)}</option>
                        </For>
                    </select>
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

                <button type="submit" disabled=move || submitting.get()>
                    {move || if submitting.get() { "Creating…" } else { "Create deployment" }}
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
        Ok(format!("Created deployment \"{}\" in namespace \"{}\".", created.name, created.namespace))
    } else {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let message = body.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error");
        Err(format!("Failed to create deployment: {message}"))
    }
}
