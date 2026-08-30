use common::{CreateUserRequest, ResetPasswordRequest, Role, UserInfo};
use gloo_net::http::Request;
use leptos::prelude::*;
use leptos::tachys::dom::event_target_value;
use leptos::task::spawn_local;

#[component]
pub fn UsersTab() -> impl IntoView {
    let users: RwSignal<Vec<UserInfo>> = RwSignal::new(Vec::new());
    let list_error = RwSignal::new(None::<String>);

    let username = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let role = RwSignal::new("user".to_string());

    let saving = RwSignal::new(false);
    let form_result: RwSignal<Option<Result<String, String>>> = RwSignal::new(None);

    let reset_target: RwSignal<Option<(i32, String)>> = RwSignal::new(None);
    let reset_password_value = RwSignal::new(String::new());
    let reset_saving = RwSignal::new(false);
    let reset_result: RwSignal<Option<Result<String, String>>> = RwSignal::new(None);

    let refresh = move || {
        spawn_local(async move {
            match Request::get("/api/users").send().await {
                Ok(resp) if resp.ok() => match resp.json::<Vec<UserInfo>>().await {
                    Ok(list) => {
                        list_error.set(None);
                        users.set(list);
                    }
                    Err(err) => list_error.set(Some(format!("failed to parse user list: {err}"))),
                },
                Ok(resp) => list_error.set(Some(format!("failed to load users: HTTP {}", resp.status()))),
                Err(err) => list_error.set(Some(format!("failed to load users: {err}"))),
            }
        });
    };
    refresh();

    let delete_user = move |id: i32| {
        let confirmed = web_sys::window()
            .and_then(|w| w.confirm_with_message("Delete this user? This can't be undone.").ok())
            .unwrap_or(false);
        if !confirmed {
            return;
        }
        spawn_local(async move {
            match Request::delete(&format!("/api/users/{id}")).send().await {
                Ok(resp) if resp.ok() => refresh(),
                Ok(resp) => {
                    let body: serde_json::Value = resp.json().await.unwrap_or_default();
                    let message = body.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error");
                    list_error.set(Some(format!("failed to delete user: {message}")));
                }
                Err(err) => list_error.set(Some(format!("failed to delete user: {err}"))),
            }
        });
    };

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        if saving.get() {
            return;
        }
        let req = CreateUserRequest {
            username: username.get().trim().to_string(),
            password: password.get(),
            role: if role.get() == "admin" { Role::Admin } else { Role::User },
        };
        saving.set(true);
        form_result.set(None);
        spawn_local(async move {
            let outcome = create(req).await;
            saving.set(false);
            match outcome {
                Ok(msg) => {
                    form_result.set(Some(Ok(msg)));
                    username.set(String::new());
                    password.set(String::new());
                    role.set("user".to_string());
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
                            <th>"Username"</th>
                            <th>"Role"</th>
                            <th></th>
                        </tr>
                    </thead>
                    <tbody>
                        <For each=move || users.get() key=|u| u.id let(u)>
                            {
                                let id = u.id;
                                let role_label = if u.role == Role::Admin { "admin" } else { "user" };
                                view! {
                                    <tr>
                                        <td>{u.username.clone()}</td>
                                        <td>{role_label}</td>
                                        <td class="table-actions">
                                            <button
                                                type="button"
                                                class="icon-button"
                                                on:click=move |_| {
                                                    reset_target.set(Some((id, u.username.clone())));
                                                    reset_password_value.set(String::new());
                                                    reset_result.set(None);
                                                }
                                            >
                                                "Reset password"
                                            </button>
                                            <button type="button" class="icon-button" on:click=move |_| delete_user(id)>
                                                "Delete"
                                            </button>
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
                reset_target
                    .get()
                    .map(|(id, target_username)| {
                        let on_reset_submit = move |ev: web_sys::SubmitEvent| {
                            ev.prevent_default();
                            if reset_saving.get() {
                                return;
                            }
                            let password = reset_password_value.get();
                            reset_saving.set(true);
                            reset_result.set(None);
                            spawn_local(async move {
                                let outcome = reset_password(id, password).await;
                                reset_saving.set(false);
                                match outcome {
                                    Ok(msg) => {
                                        reset_result.set(Some(Ok(msg)));
                                        reset_target.set(None);
                                    }
                                    Err(err) => reset_result.set(Some(Err(err))),
                                }
                            });
                        };
                        view! {
                            <h3 class="section-heading">{format!("Reset password for \"{target_username}\"")}</h3>
                            <form class="deploy-form" on:submit=on_reset_submit>
                                <label>
                                    "New password"
                                    <input
                                        type="password"
                                        required=true
                                        minlength="8"
                                        prop:value=move || reset_password_value.get()
                                        on:input=move |ev| reset_password_value.set(event_target_value(&ev))
                                    />
                                </label>
                                <div class="form-actions">
                                    <button type="submit" disabled=move || reset_saving.get()>
                                        {move || if reset_saving.get() { "Saving…" } else { "Set password" }}
                                    </button>
                                    <button type="button" class="secondary-button" on:click=move |_| reset_target.set(None)>
                                        "Cancel"
                                    </button>
                                </div>
                            </form>
                        }
                    })
            }}
            {move || {
                reset_result.get().map(|res| match res {
                    Ok(msg) => view! { <div class="success">{msg}</div> }.into_any(),
                    Err(msg) => view! { <div class="error">{msg}</div> }.into_any(),
                })
            }}

            <h3 class="section-heading">"New user"</h3>
            <form class="deploy-form" on:submit=on_submit>
                <label>
                    "Username"
                    <input
                        type="text"
                        required=true
                        minlength="3"
                        maxlength="32"
                        prop:value=move || username.get()
                        on:input=move |ev| username.set(event_target_value(&ev))
                    />
                </label>
                <label>
                    "Password"
                    <input
                        type="password"
                        required=true
                        minlength="8"
                        prop:value=move || password.get()
                        on:input=move |ev| password.set(event_target_value(&ev))
                    />
                </label>
                <label>
                    "Role"
                    <select prop:value=move || role.get() on:change=move |ev| role.set(event_target_value(&ev))>
                        <option value="user">"User"</option>
                        <option value="admin">"Admin"</option>
                    </select>
                </label>
                <button type="submit" disabled=move || saving.get()>
                    {move || if saving.get() { "Creating…" } else { "Create user" }}
                </button>
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

async fn reset_password(id: i32, password: String) -> Result<String, String> {
    let resp = Request::put(&format!("/api/users/{id}/password"))
        .json(&ResetPasswordRequest { password })
        .map_err(|err| format!("failed to encode request: {err}"))?
        .send()
        .await
        .map_err(|err| format!("request failed: {err}"))?;

    if resp.ok() {
        Ok("Password reset — that account's existing sessions have been logged out.".to_string())
    } else {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let message = body.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error");
        Err(format!("Failed to reset password: {message}"))
    }
}

async fn create(req: CreateUserRequest) -> Result<String, String> {
    let resp = Request::post("/api/users")
        .json(&req)
        .map_err(|err| format!("failed to encode request: {err}"))?
        .send()
        .await
        .map_err(|err| format!("request failed: {err}"))?;

    if resp.ok() {
        let created: UserInfo = resp.json().await.map_err(|err| format!("failed to parse response: {err}"))?;
        Ok(format!("Created user \"{}\".", created.username))
    } else {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let message = body.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error");
        Err(format!("Failed to create user: {message}"))
    }
}
