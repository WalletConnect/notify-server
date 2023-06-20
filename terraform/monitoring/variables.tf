variable "environment" {
  type = string
}

variable "app_name" {
  type = string
}

variable "prometheus_workspace_id" {
  description = "The workspace ID for the Prometheus workspace."
  type        = string
}

variable "target_group_arn" {
  description = "The ARN of the target group."
  type        = string
}

variable "load_balancer_arn" {
  description = "The ARN of the load balancer."
  type        = string
}

variable "docdb_cluster_id" {
  description = "The ID of the DocumentDB cluster."
  type        = string
}
