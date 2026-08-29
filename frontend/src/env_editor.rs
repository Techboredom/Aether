use leptos::prelude::*;
use leptos::tachys::dom::event_target_value;

/// One editable key/value row. `id` is a stable identity independent of
/// position, so removing a row from the middle doesn't scramble the others.
#[derive(Clone, Copy)]
pub struct EnvRow {
    pub id: usize,
    pub key: RwSignal<String>,
    pub value: RwSignal<String>,
}

/// A reorderable list of env-var rows, shared by the Launch form and the
/// Templates admin form.
#[derive(Clone, Copy)]
pub struct EnvVars {
    rows: RwSignal<Vec<EnvRow>>,
    next_id: RwSignal<usize>,
}

impl EnvVars {
    pub fn new() -> Self {
        Self { rows: RwSignal::new(Vec::new()), next_id: RwSignal::new(0) }
    }

    pub fn rows(&self) -> RwSignal<Vec<EnvRow>> {
        self.rows
    }

    /// Replaces the whole list, e.g. when a template is selected.
    pub fn set_from(&self, pairs: &[(String, String)]) {
        let rows: Vec<EnvRow> = pairs
            .iter()
            .enumerate()
            .map(|(id, (k, v))| EnvRow { id, key: RwSignal::new(k.clone()), value: RwSignal::new(v.clone()) })
            .collect();
        self.next_id.set(rows.len());
        self.rows.set(rows);
    }

    pub fn to_pairs(self) -> Vec<(String, String)> {
        self.rows.get().iter().map(|r| (r.key.get(), r.value.get())).collect()
    }

    pub fn add(&self) {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        self.rows.update(|rows| {
            rows.push(EnvRow { id, key: RwSignal::new(String::new()), value: RwSignal::new(String::new()) })
        });
    }

    pub fn remove(&self, id: usize) {
        self.rows.update(|rows| rows.retain(|r| r.id != id));
    }
}

impl Default for EnvVars {
    fn default() -> Self {
        Self::new()
    }
}

#[component]
pub fn EnvVarsEditor(vars: EnvVars) -> impl IntoView {
    view! {
        <div class="env-editor">
            <For each=move || vars.rows().get() key=|r| r.id let(row)>
                <div class="env-row">
                    <label>
                        "Key"
                        <input
                            type="text"
                            placeholder="e.g. PASSWORD"
                            prop:value=move || row.key.get()
                            on:input=move |ev| row.key.set(event_target_value(&ev))
                        />
                    </label>
                    <label>
                        "Value"
                        <input
                            type="text"
                            prop:value=move || row.value.get()
                            on:input=move |ev| row.value.set(event_target_value(&ev))
                        />
                    </label>
                    <button type="button" class="icon-button" title="Remove" on:click=move |_| vars.remove(row.id)>
                        "✕"
                    </button>
                </div>
            </For>
            <button type="button" class="add-row-button" on:click=move |_| vars.add()>
                "+ Add variable"
            </button>
        </div>
    }
}
