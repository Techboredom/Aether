use common::{LaunchLogEntry, SessionLogEntry};
use gloo_net::http::Request;
use leptos::prelude::*;
use leptos::task::spawn_local;

/// Login and launch history — everyone's, for an admin (with a Username
/// column); only your own, for a `user` account (same visibility split as
/// the Pods tab). Kept for support/metrics: "when did this user last log
/// in, from where" and "who launched JupyterLab with what resources."
#[component]
pub fn ActivityTab(is_admin: bool) -> impl IntoView {
    let sessions: RwSignal<Vec<SessionLogEntry>> = RwSignal::new(Vec::new());
    let sessions_error = RwSignal::new(None::<String>);
    let launches: RwSignal<Vec<LaunchLogEntry>> = RwSignal::new(Vec::new());
    let launches_error = RwSignal::new(None::<String>);

    spawn_local(async move {
        match Request::get("/api/sessions").send().await {
            Ok(resp) if resp.ok() => match resp.json::<Vec<SessionLogEntry>>().await {
                Ok(list) => sessions.set(list),
                Err(err) => sessions_error.set(Some(format!("failed to parse login history: {err}"))),
            },
            Ok(resp) => sessions_error.set(Some(format!("failed to load login history: HTTP {}", resp.status()))),
            Err(err) => sessions_error.set(Some(format!("failed to load login history: {err}"))),
        }
    });

    spawn_local(async move {
        match Request::get("/api/launches").send().await {
            Ok(resp) if resp.ok() => match resp.json::<Vec<LaunchLogEntry>>().await {
                Ok(list) => launches.set(list),
                Err(err) => launches_error.set(Some(format!("failed to parse launch history: {err}"))),
            },
            Ok(resp) => launches_error.set(Some(format!("failed to load launch history: HTTP {}", resp.status()))),
            Err(err) => launches_error.set(Some(format!("failed to load launch history: {err}"))),
        }
    });

    view! {
        <div class="tab-panel">
            <h3 class="section-heading">"Recent logins"</h3>
            {move || sessions_error.get().map(|msg| view! { <div class="error">{msg}</div> })}
            <div class="table-wrap">
                <table>
                    <thead>
                        <tr>
                            <th class:hidden=!is_admin>"Username"</th>
                            <th>"When"</th>
                            <th>"IP address"</th>
                            <th>"Browser"</th>
                        </tr>
                    </thead>
                    <tbody>
                        <For each=move || sessions.get() key=|s| (s.username.clone(), s.created_at.clone()) let(s)>
                            <tr>
                                <td class:hidden=!is_admin>{s.username}</td>
                                <td>{s.created_at}</td>
                                <td>{s.ip_address.unwrap_or_else(|| "—".into())}</td>
                                <td>{s.user_agent.unwrap_or_else(|| "—".into())}</td>
                            </tr>
                        </For>
                    </tbody>
                </table>
                <Show when=move || sessions.get().is_empty() && sessions_error.get().is_none()>
                    <p class="empty">"No login history yet."</p>
                </Show>
            </div>

            <h3 class="section-heading">"Recent launches"</h3>
            {move || launches_error.get().map(|msg| view! { <div class="error">{msg}</div> })}
            <div class="table-wrap">
                <table>
                    <thead>
                        <tr>
                            <th class:hidden=!is_admin>"Username"</th>
                            <th>"When"</th>
                            <th>"Name"</th>
                            <th>"Template"</th>
                            <th>"Image"</th>
                            <th>"Resources"</th>
                            <th>"Args"</th>
                        </tr>
                    </thead>
                    <tbody>
                        <For each=move || launches.get() key=|l| (l.username.clone(), l.deployment_name.clone(), l.created_at.clone()) let(l)>
                            {
                                let resources = resource_summary(&l);
                                view! {
                                    <tr>
                                        <td class:hidden=!is_admin>{l.username}</td>
                                        <td>{l.created_at}</td>
                                        <td>{l.deployment_name}</td>
                                        <td>{l.template_name.unwrap_or_else(|| "Custom".into())}</td>
                                        <td>{l.image}</td>
                                        <td>{resources}</td>
                                        <td>{l.args.join(" ")}</td>
                                    </tr>
                                }
                            }
                        </For>
                    </tbody>
                </table>
                <Show when=move || launches.get().is_empty() && launches_error.get().is_none()>
                    <p class="empty">"No launch history yet."</p>
                </Show>
            </div>
        </div>
    }
}

fn resource_summary(l: &LaunchLogEntry) -> String {
    let mut parts = Vec::new();
    if let (Some(req), Some(lim)) = (&l.cpu_request, &l.cpu_limit) {
        parts.push(format!("CPU {req}/{lim}"));
    }
    if let (Some(req), Some(lim)) = (&l.memory_request, &l.memory_limit) {
        parts.push(format!("Mem {req}/{lim}"));
    }
    if let (Some(accel), Some(count)) = (&l.accelerator_type, l.accelerator_count) {
        parts.push(format!("{accel} x{count}"));
    }
    if parts.is_empty() { "—".to_string() } else { parts.join(", ") }
}
