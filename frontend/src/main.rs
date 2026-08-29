mod create_deployment_tab;
mod env_editor;
mod format;
mod login;
mod pod_detail;
mod pods_tab;
mod templates_tab;
mod users_tab;
mod ws;

use common::{Role, UserInfo};
use create_deployment_tab::CreateDeploymentTab;
use gloo_net::http::Request;
use leptos::prelude::*;
use leptos::task::spawn_local;
use login::LoginPage;
use pods_tab::PodsTab;
use templates_tab::TemplatesTab;
use users_tab::UsersTab;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Pods,
    Launch,
    Templates,
    Users,
}

#[component]
fn App() -> impl IntoView {
    let current_user: RwSignal<Option<UserInfo>> = RwSignal::new(None);
    let checking = RwSignal::new(true);

    spawn_local(async move {
        if let Ok(resp) = Request::get("/api/me").send().await
            && resp.ok()
                && let Ok(user) = resp.json::<UserInfo>().await {
                    current_user.set(Some(user));
                }
        checking.set(false);
    });

    view! {
        {move || {
            if checking.get() {
                view! { <div class="loading-screen">"Loading…"</div> }.into_any()
            } else {
                match current_user.get() {
                    None => view! { <LoginPage current_user=current_user /> }.into_any(),
                    Some(user) => view! { <AppShell user=user current_user=current_user /> }.into_any(),
                }
            }
        }}
    }
}

#[component]
fn AppShell(user: UserInfo, current_user: RwSignal<Option<UserInfo>>) -> impl IntoView {
    let tab = RwSignal::new(Tab::Pods);
    let is_admin = user.role == Role::Admin;
    let username = user.username.clone();

    let logout = move |_| {
        spawn_local(async move {
            let _ = Request::post("/api/logout").send().await;
            current_user.set(None);
        });
    };

    view! {
        <main>
            <header>
                <h1>"Aether"</h1>
                <nav class="tabs">
                    <button
                        class="tab-button"
                        class:active=move || tab.get() == Tab::Pods
                        on:click=move |_| tab.set(Tab::Pods)
                    >
                        "Pods"
                    </button>
                    <button
                        class="tab-button"
                        class:active=move || tab.get() == Tab::Launch
                        on:click=move |_| tab.set(Tab::Launch)
                    >
                        "Launch"
                    </button>
                    <Show when=move || is_admin>
                        <button
                            class="tab-button"
                            class:active=move || tab.get() == Tab::Templates
                            on:click=move |_| tab.set(Tab::Templates)
                        >
                            "Templates"
                        </button>
                        <button
                            class="tab-button"
                            class:active=move || tab.get() == Tab::Users
                            on:click=move |_| tab.set(Tab::Users)
                        >
                            "Users"
                        </button>
                    </Show>
                </nav>
                <div class="user-info">
                    <span class="username">{username}</span>
                    <button class="icon-button" on:click=logout>
                        "Log out"
                    </button>
                </div>
            </header>

            <div class:hidden=move || tab.get() != Tab::Pods>
                <PodsTab is_admin=is_admin />
            </div>
            <div class:hidden=move || tab.get() != Tab::Launch>
                <CreateDeploymentTab />
            </div>
            <Show when=move || is_admin>
                <div class:hidden=move || tab.get() != Tab::Templates>
                    <TemplatesTab />
                </div>
                <div class:hidden=move || tab.get() != Tab::Users>
                    <UsersTab />
                </div>
            </Show>
        </main>
    }
}
