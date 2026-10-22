# The web app: traefik/whoami, 3 replicas spread over the cluster's machines,
# behind a service and the cluster's ingress (k3s comes with Traefik), at /.

terraform {
  required_providers {
    kubernetes = {
      source  = "hashicorp/kubernetes"
      version = "~> 2.38"
    }
  }
}

variable "kubeconfig" {
  type        = string
  description = "The cluster's kubeconfig file"
}

provider "kubernetes" {
  config_path = var.kubeconfig
}

locals {
  labels = { app = "whoami" }
}

resource "kubernetes_namespace_v1" "web" {
  metadata {
    name = "web"
  }
}

resource "kubernetes_deployment_v1" "whoami" {
  metadata {
    name      = "whoami"
    namespace = kubernetes_namespace_v1.web.metadata[0].name
  }
  spec {
    replicas = 3
    selector {
      match_labels = local.labels
    }
    template {
      metadata {
        labels = local.labels
      }
      spec {
        # one on each machine, when there are machines enough
        topology_spread_constraint {
          max_skew           = 1
          topology_key       = "kubernetes.io/hostname"
          when_unsatisfiable = "ScheduleAnyway"
          label_selector {
            match_labels = local.labels
          }
        }
        container {
          name  = "whoami"
          image = "traefik/whoami:v1.10"
          port {
            container_port = 80
          }
        }
      }
    }
  }
}

resource "kubernetes_service_v1" "whoami" {
  metadata {
    name      = "whoami"
    namespace = kubernetes_namespace_v1.web.metadata[0].name
  }
  spec {
    selector = local.labels
    port {
      port        = 80
      target_port = 80
    }
  }
}

resource "kubernetes_ingress_v1" "whoami" {
  metadata {
    name      = "whoami"
    namespace = kubernetes_namespace_v1.web.metadata[0].name
  }
  spec {
    rule {
      http {
        path {
          path      = "/"
          path_type = "Prefix"
          backend {
            service {
              name = kubernetes_service_v1.whoami.metadata[0].name
              port {
                number = 80
              }
            }
          }
        }
      }
    }
  }
}
