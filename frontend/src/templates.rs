/// A curated starting point for a common workload type. Selecting one in the
/// launcher pre-fills the form; every field stays editable afterward.
///
/// There's no ingress controller or StorageClass in the cluster yet, so these
/// are all stateless single-container apps reachable via their own
/// LoadBalancer Service — no persistent volumes, no shared hostname/routing.
pub struct Template {
    pub id: &'static str,
    pub label: &'static str,
    pub image: &'static str,
    pub port: Option<i32>,
    pub cpu_request: &'static str,
    pub cpu_limit: &'static str,
    pub memory_request: &'static str,
    pub memory_limit: &'static str,
    pub accelerator_type: &'static str,
    pub accelerator_count: i64,
    /// Env var names to scaffold in the form, with blank values for the user to fill in.
    pub env_keys: &'static [&'static str],
    /// Placeholder command-line arguments the user should edit before launching.
    pub args: &'static [&'static str],
    pub notes: &'static str,
}

pub const CUSTOM_TEMPLATE_ID: &str = "custom";

pub const TEMPLATES: &[Template] = &[
    Template {
        id: "ollama",
        label: "Ollama",
        image: "ollama/ollama:latest",
        port: Some(11434),
        cpu_request: "500m",
        cpu_limit: "2",
        memory_request: "2Gi",
        memory_limit: "8Gi",
        accelerator_type: "amd.com/gpu",
        accelerator_count: 1,
        env_keys: &[],
        args: &[],
        notes: "No authentication by default — anyone who can reach the Service can use it. Pull models via the Ollama API/CLI after it starts; there's no persistent storage yet, so pulled models are lost if the pod restarts.",
    },
    Template {
        id: "vllm",
        label: "vLLM",
        image: "vllm/vllm-openai:latest",
        port: Some(8000),
        cpu_request: "1",
        cpu_limit: "4",
        memory_request: "4Gi",
        memory_limit: "16Gi",
        accelerator_type: "amd.com/gpu",
        accelerator_count: 1,
        env_keys: &[],
        args: &["--model=<huggingface-model-id>"],
        notes: "Edit the --model argument below to the Hugging Face model you want to serve. No persistent storage yet, so the model is re-downloaded on every restart.",
    },
    Template {
        id: "sglang",
        label: "SGLang",
        image: "lmsysorg/sglang:latest",
        port: Some(30000),
        cpu_request: "1",
        cpu_limit: "4",
        memory_request: "4Gi",
        memory_limit: "16Gi",
        accelerator_type: "amd.com/gpu",
        accelerator_count: 1,
        env_keys: &[],
        args: &["--model-path=<huggingface-model-id>", "--host=0.0.0.0", "--port=30000"],
        notes: "Edit --model-path to the Hugging Face model you want to serve. No persistent storage yet, so the model is re-downloaded on every restart.",
    },
    Template {
        id: "jupyterlab",
        label: "JupyterLab",
        image: "jupyter/base-notebook:latest",
        port: Some(8888),
        cpu_request: "250m",
        cpu_limit: "2",
        memory_request: "512Mi",
        memory_limit: "4Gi",
        accelerator_type: "",
        accelerator_count: 0,
        env_keys: &["JUPYTER_TOKEN"],
        args: &[],
        notes: "Set JUPYTER_TOKEN to choose your own login token, or leave it blank and read the auto-generated one from the pod's logs after it starts (open the pod's detail panel on the Pods tab).",
    },
    Template {
        id: "rstudio",
        label: "RStudio",
        image: "rocker/rstudio:latest",
        port: Some(8787),
        cpu_request: "250m",
        cpu_limit: "2",
        memory_request: "512Mi",
        memory_limit: "4Gi",
        accelerator_type: "",
        accelerator_count: 0,
        env_keys: &["PASSWORD"],
        args: &[],
        notes: "Username is always \"rstudio\". Set PASSWORD to choose your own login password, or leave it blank and read the auto-generated one from the pod's logs after it starts.",
    },
];
