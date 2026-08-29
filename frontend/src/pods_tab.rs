use std::collections::HashMap;

use common::PodInfo;
use gloo_net::http::Request;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::pod_detail::PodDetailPanel;
use crate::{format, ws};

#[component]
pub fn PodsTab(is_admin: bool) -> impl IntoView {
    let pods: RwSignal<HashMap<String, PodInfo>> = RwSignal::new(HashMap::new());
    let connected = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let selected_pod: RwSignal<Option<String>> = RwSignal::new(None);

    spawn_local(async move {
        match Request::get("/api/pods").send().await {
            Ok(resp) if resp.ok() => match resp.json::<Vec<PodInfo>>().await {
                Ok(list) => pods.update(|map| {
                    for pod in list {
                        map.insert(pod.name.clone(), pod);
                    }
                }),
                Err(err) => error.set(Some(format!("failed to parse initial pod list: {err}"))),
            },
            Ok(resp) => error.set(Some(format!("failed to load pods: HTTP {}", resp.status()))),
            Err(err) => error.set(Some(format!("failed to load pods: {err}"))),
        }
        ws::run(pods, connected, error).await;
    });

    let rows = Memo::new(move |_| {
        let mut list: Vec<PodInfo> = pods.get().into_values().collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    });

    let total = Memo::new(move |_| rows.get().len());
    let running = Memo::new(move |_| rows.get().iter().filter(|p| p.phase == "Running").count());
    let pending = Memo::new(move |_| rows.get().iter().filter(|p| p.phase == "Pending").count());
    let failed = Memo::new(move |_| rows.get().iter().filter(|p| p.phase == "Failed").count());

    view! {
        <div class="tab-panel">
            <div class="panel-header">
                <span class="status" class:live=move || connected.get()>
                    {move || if connected.get() { "live" } else { "reconnecting…" }}
                </span>
            </div>

            {move || {
                error
                    .get()
                    .map(|msg| view! { <div class="error">{msg}</div> })
            }}

            <div class="stats">
                <div class="stat-tile">
                    <span class="stat-value">{total}</span>
                    <span class="stat-label">"Total pods"</span>
                </div>
                <div class="stat-tile">
                    <span class="stat-value">
                        <span class="stat-dot good"></span>
                        {running}
                    </span>
                    <span class="stat-label">"Running"</span>
                </div>
                <div class="stat-tile">
                    <span class="stat-value">
                        <span class="stat-dot warning"></span>
                        {pending}
                    </span>
                    <span class="stat-label">"Pending"</span>
                </div>
                <div class="stat-tile">
                    <span class="stat-value">
                        <span class="stat-dot critical"></span>
                        {failed}
                    </span>
                    <span class="stat-label">"Failed"</span>
                </div>
            </div>

            <div class="table-wrap">
                <table>
                    <thead>
                        <tr>
                            <th>"Name"</th>
                            <th class:hidden=!is_admin>"Owner"</th>
                            <th>"Status"</th>
                            <th>"Node"</th>
                            <th>"Restarts"</th>
                            <th>"Age"</th>
                            <th>"CPU request"</th>
                            <th>"CPU limit"</th>
                            <th>"Memory request"</th>
                            <th>"Memory limit"</th>
                            <th>"Accelerators"</th>
                            <th>"Credential"</th>
                        </tr>
                    </thead>
                    <tbody>
                        <For each=move || rows.get() key=|pod| pod.name.clone() let(pod)>
                            <PodRow pod=pod selected_pod=selected_pod is_admin=is_admin />
                        </For>
                    </tbody>
                </table>

                <Show when=move || rows.get().is_empty()>
                    <p class="empty">"No pods found in this namespace."</p>
                </Show>
            </div>

            {move || {
                selected_pod
                    .get()
                    .map(|name| view! { <PodDetailPanel pods=pods name=name selected=selected_pod /> })
            }}
        </div>
    }
}

#[component]
fn PodRow(pod: PodInfo, selected_pod: RwSignal<Option<String>>, is_admin: bool) -> impl IntoView {
    let ready = format!("{}/{}", pod.ready_containers, pod.total_containers);
    let accelerators = format::accelerators(&pod.accelerators);
    let badge_class = format!("badge {}", format::phase_class(&pod.phase));
    let reason = format::pod_reason(&pod.containers);
    let row_name = pod.name.clone();
    let owner = pod.owner.clone().unwrap_or_else(|| "—".into());
    let credential = pod.credential.clone();
    let proxy_path = pod.proxy_path.clone();

    view! {
        <tr class="clickable-row" on:click=move |_| selected_pod.set(Some(row_name.clone()))>
            <td>{pod.name.clone()}</td>
            <td class:hidden=!is_admin>{owner}</td>
            <td>
                <span class=badge_class>{pod.phase.clone()}</span>
                " "
                <span class="ready">{ready}</span>
                {reason.map(|r| view! { <div class="reason-hint">{r}</div> })}
            </td>
            <td>{pod.node.clone().unwrap_or_else(|| "—".into())}</td>
            <td>{pod.restarts}</td>
            <td>{format::age(pod.start_time.as_deref())}</td>
            <td>{format::millicores(pod.cpu_request_millicores)}</td>
            <td>{format::millicores(pod.cpu_limit_millicores)}</td>
            <td>{format::bytes(pod.memory_request_bytes)}</td>
            <td>{format::bytes(pod.memory_limit_bytes)}</td>
            <td>{accelerators}</td>
            <td>
                {match credential {
                    Some(cred) => {
                        view! {
                            <div class="credential">
                                {proxy_path.map(|path| {
                                    view! {
                                        <a
                                            class="icon-button"
                                            href=path
                                            target="_blank"
                                            title="Open — already logged in, no token needed"
                                            on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                                        >
                                            "Open"
                                        </a>
                                    }
                                })}
                                <span class="credential-key">{cred.env_key}</span>
                                <code class="credential-value" title="Click to select, then copy">
                                    {cred.value}
                                </code>
                            </div>
                        }
                            .into_any()
                    }
                    None => view! { "—" }.into_any(),
                }}
            </td>
        </tr>
    }
}
