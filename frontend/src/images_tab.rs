use common::{ImageEntry, SaveImageRequest};
use gloo_net::http::Request;
use leptos::prelude::*;
use leptos::tachys::dom::event_target_value;
use leptos::task::spawn_local;

#[component]
pub fn ImagesTab() -> impl IntoView {
    let images: RwSignal<Vec<ImageEntry>> = RwSignal::new(Vec::new());
    let list_error = RwSignal::new(None::<String>);

    let editing_id = RwSignal::new(None::<i32>);
    let name = RwSignal::new(String::new());
    let image = RwSignal::new(String::new());
    let description = RwSignal::new(String::new());

    let saving = RwSignal::new(false);
    let form_result: RwSignal<Option<Result<String, String>>> = RwSignal::new(None);

    let refresh = move || {
        spawn_local(async move {
            match Request::get("/api/images").send().await {
                Ok(resp) if resp.ok() => match resp.json::<Vec<ImageEntry>>().await {
                    Ok(list) => {
                        list_error.set(None);
                        images.set(list);
                    }
                    Err(err) => list_error.set(Some(format!("failed to parse image list: {err}"))),
                },
                Ok(resp) => list_error.set(Some(format!("failed to load images: HTTP {}", resp.status()))),
                Err(err) => list_error.set(Some(format!("failed to load images: {err}"))),
            }
        });
    };
    refresh();

    let clear_form = move || {
        editing_id.set(None);
        name.set(String::new());
        image.set(String::new());
        description.set(String::new());
    };

    let load_into_form = move |i: &ImageEntry| {
        editing_id.set(Some(i.id));
        name.set(i.name.clone());
        image.set(i.image.clone());
        description.set(i.description.clone());
        form_result.set(None);
    };

    let delete_image = move |id: i32| {
        let confirmed = web_sys::window()
            .and_then(|w| w.confirm_with_message("Delete this catalog entry? This can't be undone.").ok())
            .unwrap_or(false);
        if !confirmed {
            return;
        }
        spawn_local(async move {
            let outcome = Request::delete(&format!("/api/images/{id}")).send().await;
            match outcome {
                Ok(resp) if resp.ok() => {
                    if editing_id.get() == Some(id) {
                        clear_form();
                    }
                    refresh();
                }
                Ok(resp) => list_error.set(Some(format!("failed to delete image: HTTP {}", resp.status()))),
                Err(err) => list_error.set(Some(format!("failed to delete image: {err}"))),
            }
        });
    };

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        if saving.get() {
            return;
        }

        let req = SaveImageRequest {
            name: name.get().trim().to_string(),
            image: image.get().trim().to_string(),
            description: description.get().trim().to_string(),
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
                            <th>"Description"</th>
                            <th></th>
                        </tr>
                    </thead>
                    <tbody>
                        <For each=move || images.get() key=|i| i.id let(i)>
                            {
                                let i_for_edit = i.clone();
                                let id = i.id;
                                view! {
                                    <tr>
                                        <td>{i.name.clone()}</td>
                                        <td>{i.image.clone()}</td>
                                        <td>{i.description.clone()}</td>
                                        <td class="table-actions">
                                            <button type="button" class="icon-button" on:click=move |_| load_into_form(&i_for_edit)>
                                                "Edit"
                                            </button>
                                            <button type="button" class="icon-button" on:click=move |_| delete_image(id)>
                                                "Delete"
                                            </button>
                                        </td>
                                    </tr>
                                }
                            }
                        </For>
                    </tbody>
                </table>
                <Show when=move || images.get().is_empty() && list_error.get().is_none()>
                    <p class="empty">"No catalog entries yet — add one below."</p>
                </Show>
            </div>

            <h3 class="section-heading">
                {move || if editing_id.get().is_some() { "Edit image" } else { "New image" }}
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
                    "Description (optional)"
                    <textarea
                        rows="2"
                        maxlength="500"
                        prop:value=move || description.get()
                        on:input=move |ev| description.set(event_target_value(&ev))
                    ></textarea>
                </label>

                <div class="form-actions">
                    <button type="submit" disabled=move || saving.get()>
                        {move || {
                            if saving.get() {
                                "Saving…"
                            } else if editing_id.get().is_some() {
                                "Save changes"
                            } else {
                                "Add image"
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

async fn save(id: Option<i32>, req: SaveImageRequest) -> Result<String, String> {
    let builder = match id {
        Some(id) => Request::put(&format!("/api/images/{id}")),
        None => Request::post("/api/images"),
    };
    let resp = builder
        .json(&req)
        .map_err(|err| format!("failed to encode request: {err}"))?
        .send()
        .await
        .map_err(|err| format!("request failed: {err}"))?;

    if resp.ok() {
        let saved: ImageEntry = resp.json().await.map_err(|err| format!("failed to parse response: {err}"))?;
        Ok(format!("Saved image \"{}\".", saved.name))
    } else {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let message = body.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error");
        Err(format!("Failed to save image: {message}"))
    }
}
