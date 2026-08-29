use std::collections::BTreeMap;

use axum::extract::State;
use axum::Json;
use common::{CreateDeploymentRequest, CreateDeploymentResponse};
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container, EnvVar, PodSpec, PodTemplateSpec, ResourceRequirements, Service, ServicePort, ServiceSpec,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::{Api, PostParams};

use crate::error::ApiError;
use crate::state::AppState;

pub async fn create_deployment(
    State(state): State<AppState>,
    Json(req): Json<CreateDeploymentRequest>,
) -> Result<Json<CreateDeploymentResponse>, ApiError> {
    if req.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    if req.image.trim().is_empty() {
        return Err(ApiError::BadRequest("image is required".into()));
    }
    if req.replicas < 0 {
        return Err(ApiError::BadRequest("replicas must not be negative".into()));
    }

    let mut requests = BTreeMap::new();
    let mut limits = BTreeMap::new();
    if let Some(v) = &req.cpu_request {
        requests.insert("cpu".to_string(), Quantity(v.clone()));
    }
    if let Some(v) = &req.cpu_limit {
        limits.insert("cpu".to_string(), Quantity(v.clone()));
    }
    if let Some(v) = &req.memory_request {
        requests.insert("memory".to_string(), Quantity(v.clone()));
    }
    if let Some(v) = &req.memory_limit {
        limits.insert("memory".to_string(), Quantity(v.clone()));
    }
    if let (Some(accel_type), Some(count)) = (&req.accelerator_type, req.accelerator_count)
        && !accel_type.trim().is_empty() && count > 0 {
            // Extended resources like GPUs require request == limit.
            let qty = Quantity(count.to_string());
            requests.insert(accel_type.clone(), qty.clone());
            limits.insert(accel_type.clone(), qty);
        }

    let env: Vec<EnvVar> = req
        .env
        .iter()
        .filter(|(_, value)| !value.trim().is_empty())
        .map(|(name, value)| EnvVar {
            name: name.clone(),
            value: Some(value.clone()),
            ..Default::default()
        })
        .collect();
    let args: Vec<String> = req.args.iter().map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect();

    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), req.name.clone());

    let deployment = Deployment {
        metadata: ObjectMeta {
            name: Some(req.name.clone()),
            namespace: Some(state.namespace.clone()),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(req.replicas),
            selector: LabelSelector {
                match_labels: Some(labels.clone()),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels.clone()),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: req.name.clone(),
                        image: Some(req.image.clone()),
                        resources: Some(ResourceRequirements {
                            requests: (!requests.is_empty()).then_some(requests),
                            limits: (!limits.is_empty()).then_some(limits),
                            ..Default::default()
                        }),
                        env: (!env.is_empty()).then_some(env),
                        args: (!args.is_empty()).then_some(args),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        status: None,
    };

    let deployments: Api<Deployment> = Api::namespaced(state.client.clone(), &state.namespace);
    let created = deployments.create(&PostParams::default(), &deployment).await?;
    let name = created.metadata.name.unwrap_or(req.name);

    // No ingress controller in the cluster yet, so expose the app directly via
    // its own LoadBalancer Service (MetalLB assigns it an external IP).
    let mut service_name = None;
    if let Some(port) = req.container_port {
        let service = Service {
            metadata: ObjectMeta {
                name: Some(name.clone()),
                namespace: Some(state.namespace.clone()),
                labels: Some(labels.clone()),
                ..Default::default()
            },
            spec: Some(ServiceSpec {
                type_: Some("LoadBalancer".to_string()),
                selector: Some(labels),
                ports: Some(vec![ServicePort {
                    port,
                    target_port: Some(IntOrString::Int(port)),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            status: None,
        };
        let services: Api<Service> = Api::namespaced(state.client.clone(), &state.namespace);
        let created_service = services.create(&PostParams::default(), &service).await?;
        service_name = created_service.metadata.name;
    }

    Ok(Json(CreateDeploymentResponse {
        name,
        namespace: state.namespace,
        service_name,
        container_port: req.container_port,
    }))
}
