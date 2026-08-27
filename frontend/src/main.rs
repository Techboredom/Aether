mod format;
mod ws;

use std::collections::HashMap;

use common::PodInfo;
use gloo_net::http::Request;
use leptos::prelude::*;
use leptos::task::spawn_local;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let pods: RwSignal<HashMap<String, PodInfo>> = RwSignal::new(HashMap::new());
    let connected = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);

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

    view! {
        <main>
            <header>
                <h1>"Pods"</h1>
                <span class="status" class:live=move || connected.get()>
                    {move || if connected.get() { "live" } else { "reconnecting…" }}
                </span>
            </header>

            {move || {
                error
                    .get()
                    .map(|msg| view! { <div class="error">{msg}</div> })
            }}

            <table>
                <thead>
                    <tr>
                        <th>"Name"</th>
                        <th>"Status"</th>
                        <th>"Node"</th>
                        <th>"Restarts"</th>
                        <th>"Age"</th>
                        <th>"CPU request"</th>
                        <th>"CPU limit"</th>
                        <th>"Memory request"</th>
                        <th>"Memory limit"</th>
                        <th>"Accelerators"</th>
                    </tr>
                </thead>
                <tbody>
                    <For each=move || rows.get() key=|pod| pod.name.clone() let(pod)>
                        <PodRow pod=pod />
                    </For>
                </tbody>
            </table>

            <Show when=move || rows.get().is_empty()>
                <p class="empty">"No pods found in this namespace."</p>
            </Show>
        </main>
    }
}

#[component]
fn PodRow(pod: PodInfo) -> impl IntoView {
    let ready = format!("{}/{}", pod.ready_containers, pod.total_containers);
    let accelerators = format::accelerators(&pod.accelerators);

    view! {
        <tr>
            <td>{pod.name.clone()}</td>
            <td>
                <span class="phase" data-phase=pod.phase.clone()>
                    {pod.phase.clone()}
                </span>
                " "
                <span class="ready">{ready}</span>
            </td>
            <td>{pod.node.clone().unwrap_or_else(|| "—".into())}</td>
            <td>{pod.restarts}</td>
            <td>{format::age(pod.start_time.as_deref())}</td>
            <td>{format::millicores(pod.cpu_request_millicores)}</td>
            <td>{format::millicores(pod.cpu_limit_millicores)}</td>
            <td>{format::bytes(pod.memory_request_bytes)}</td>
            <td>{format::bytes(pod.memory_limit_bytes)}</td>
            <td>{accelerators}</td>
        </tr>
    }
}
