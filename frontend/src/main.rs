mod create_deployment_tab;
mod env_editor;
mod format;
mod pod_detail;
mod pods_tab;
mod templates_tab;
mod ws;

use create_deployment_tab::CreateDeploymentTab;
use leptos::prelude::*;
use pods_tab::PodsTab;
use templates_tab::TemplatesTab;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Pods,
    Launch,
    Templates,
}

#[component]
fn App() -> impl IntoView {
    let tab = RwSignal::new(Tab::Pods);

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
                    <button
                        class="tab-button"
                        class:active=move || tab.get() == Tab::Templates
                        on:click=move |_| tab.set(Tab::Templates)
                    >
                        "Templates"
                    </button>
                </nav>
            </header>

            <div class:hidden=move || tab.get() != Tab::Pods>
                <PodsTab />
            </div>
            <div class:hidden=move || tab.get() != Tab::Launch>
                <CreateDeploymentTab />
            </div>
            <div class:hidden=move || tab.get() != Tab::Templates>
                <TemplatesTab />
            </div>
        </main>
    }
}
