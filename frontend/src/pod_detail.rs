use std::collections::HashMap;

use common::{ContainerStatusInfo, PodEventInfo, PodInfo};
use gloo_net::http::Request;
use leptos::prelude::*;
use leptos::tachys::dom::{event_target_checked, event_target_value};
use leptos::task::spawn_local;

use crate::deployment_manage::ManageDeploymentSection;

#[component]
pub fn PodDetailPanel(
    pods: RwSignal<HashMap<String, PodInfo>>,
    name: String,
    selected: RwSignal<Option<String>>,
) -> impl IntoView {
    let pod_name = name.clone();
    let pod = Memo::new(move |_| pods.get().get(&pod_name).cloned());

    let events: RwSignal<Vec<PodEventInfo>> = RwSignal::new(Vec::new());
    let events_error = RwSignal::new(None::<String>);

    let container = RwSignal::new(String::new());
    let tail_lines = RwSignal::new("500".to_string());
    let previous = RwSignal::new(false);
    let log_text = RwSignal::new(String::new());
    let log_error = RwSignal::new(None::<String>);
    let loading_logs = RwSignal::new(false);

    {
        let pod_name = name.clone();
        spawn_local(async move {
            match Request::get(&format!("/api/pods/{pod_name}/events")).send().await {
                Ok(resp) if resp.ok() => match resp.json::<Vec<PodEventInfo>>().await {
                    Ok(list) => events.set(list),
                    Err(err) => events_error.set(Some(format!("failed to parse events: {err}"))),
                },
                Ok(resp) => events_error.set(Some(format!("failed to load events: HTTP {}", resp.status()))),
                Err(err) => events_error.set(Some(format!("failed to load events: {err}"))),
            }
        });
    }

    // Default the container selector to the pod's first container, then load its logs once.
    {
        let pod_name = name.clone();
        Effect::new(move |_| {
            if container.get_untracked().is_empty()
                && let Some(first) = pod.get().and_then(|p| p.containers.first().cloned()) {
                    container.set(first.name.clone());
                    spawn_local(fetch_logs(
                        pod_name.clone(),
                        first.name,
                        tail_lines.get_untracked(),
                        previous.get_untracked(),
                        log_text,
                        log_error,
                        loading_logs,
                    ));
                }
        });
    }

    let refresh_logs = {
        let pod_name = name.clone();
        move || {
            spawn_local(fetch_logs(
                pod_name.clone(),
                container.get(),
                tail_lines.get(),
                previous.get(),
                log_text,
                log_error,
                loading_logs,
            ));
        }
    };

    let on_container_change = {
        let pod_name = name.clone();
        move |ev: web_sys::Event| {
            let value = event_target_value(&ev);
            container.set(value.clone());
            spawn_local(fetch_logs(
                pod_name.clone(),
                value,
                tail_lines.get(),
                previous.get(),
                log_text,
                log_error,
                loading_logs,
            ));
        }
    };

    let close = move |_| selected.set(None);

    view! {
        <div class="overlay" on:click=close>
            <div class="panel" on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()>
                <div class="panel-head">
                    <h2>{name.clone()}</h2>
                    <button class="icon-button" on:click=close>
                        "✕"
                    </button>
                </div>

                <Show when=move || pod.get().is_none()>
                    <p class="empty">"This pod is no longer present."</p>
                </Show>

                {move || {
                    pod.get()
                        .map(|p| {
                            view! {
                                <section>
                                    <h3>"Containers"</h3>
                                    <ul class="container-list">
                                        <For each=move || p.containers.clone() key=|c| c.name.clone() let(c)>
                                            <ContainerStatusRow c=c />
                                        </For>
                                    </ul>
                                </section>
                            }
                        })
                }}

                {move || {
                    pod.get()
                        .and_then(|p| p.deployment_name.clone())
                        .map(|deployment_name| {
                            view! { <ManageDeploymentSection deployment_name=deployment_name selected=selected /> }
                        })
                }}

                <section>
                    <h3>"Events"</h3>
                    {move || events_error.get().map(|msg| view! { <div class="error">{msg}</div> })}
                    <Show when=move || events_error.get().is_none() && events.get().is_empty()>
                        <p class="empty">"No recent events."</p>
                    </Show>
                    <ul class="event-list">
                        <For
                            each=move || events.get()
                            key=|e| format!("{}-{}-{}-{:?}", e.reason, e.message, e.count, e.last_seen)
                            let(e)
                        >
                            <EventRow e=e />
                        </For>
                    </ul>
                </section>

                <section>
                    <h3>"Logs"</h3>
                    <div class="log-controls">
                        <select prop:value=move || container.get() on:change=on_container_change>
                            {move || {
                                pod.get()
                                    .map(|p| {
                                        p.containers
                                            .iter()
                                            .map(|c| {
                                                let n = c.name.clone();
                                                view! { <option value=n.clone()>{n.clone()}</option> }
                                            })
                                            .collect::<Vec<_>>()
                                    })
                            }}
                        </select>
                        <label class="checkbox">
                            <input
                                type="checkbox"
                                prop:checked=move || previous.get()
                                on:change=move |ev| previous.set(event_target_checked(&ev))
                            />
                            "Previous container"
                        </label>
                        <input
                            type="number"
                            min="1"
                            step="1"
                            class="tail-input"
                            prop:value=move || tail_lines.get()
                            on:input=move |ev| tail_lines.set(event_target_value(&ev))
                        />
                        <button on:click=move |_| refresh_logs() disabled=move || loading_logs.get()>
                            {move || if loading_logs.get() { "Loading…" } else { "Refresh" }}
                        </button>
                    </div>
                    {move || log_error.get().map(|msg| view! { <div class="error">{msg}</div> })}
                    <pre class="log-view">{move || log_text.get()}</pre>
                </section>
            </div>
        </div>
    }
}

#[component]
fn ContainerStatusRow(c: ContainerStatusInfo) -> impl IntoView {
    let badge_class = format!(
        "badge {}",
        match c.state.as_str() {
            "running" => "good",
            "waiting" => "warning",
            "terminated" => "critical",
            _ => "serious",
        }
    );

    view! {
        <li class="container-row">
            <div class="container-row-head">
                <span class="container-name">{c.name.clone()}</span>
                <span class=badge_class>{c.state.clone()}</span>
                <span class="ready">{format!("restarts: {}", c.restart_count)}</span>
            </div>
            {c.reason.clone().map(|r| view! { <div class="reason">{r}</div> })}
            {c.message.clone().map(|m| view! { <div class="message">{m}</div> })}
        </li>
    }
}

#[component]
fn EventRow(e: PodEventInfo) -> impl IntoView {
    let badge_class = if e.type_ == "Warning" { "badge warning" } else { "badge good" };

    view! {
        <li class="event-row">
            <span class=badge_class>{e.type_.clone()}</span>
            <span class="event-reason">{e.reason.clone()}</span>
            <span class="event-message">{e.message.clone()}</span>
            <span class="event-count">{format!("×{}", e.count)}</span>
        </li>
    }
}

async fn fetch_logs(
    pod_name: String,
    container: String,
    tail_lines: String,
    previous: bool,
    log_text: RwSignal<String>,
    log_error: RwSignal<Option<String>>,
    loading: RwSignal<bool>,
) {
    if container.is_empty() {
        return;
    }
    loading.set(true);
    log_error.set(None);

    let url = format!("/api/pods/{pod_name}/logs?container={container}&tail_lines={tail_lines}&previous={previous}");
    match Request::get(&url).send().await {
        Ok(resp) if resp.ok() => match resp.text().await {
            Ok(text) => log_text.set(text),
            Err(err) => log_error.set(Some(format!("failed to read logs: {err}"))),
        },
        Ok(resp) => {
            let body = resp.text().await.unwrap_or_default();
            log_error.set(Some(format!("failed to load logs (HTTP {}): {body}", resp.status())));
        }
        Err(err) => log_error.set(Some(format!("failed to load logs: {err}"))),
    }
    loading.set(false);
}
