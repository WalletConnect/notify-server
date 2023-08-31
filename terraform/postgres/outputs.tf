output "database_name" {
  description = "The name of the default database in the cluster"
  value       = var.db_name
}

output "master_username" {
  description = "The username for the master DB user"
  value       = var.db_master_username
}

output "rds_cluster_arn" {
  description = "The ARN of the cluster"
  value       = module.database_cluster.cluster_arn
}

output "rds_cluster_id" {
  description = "The ID of the cluster"
  value       = module.database_cluster.cluster_id
}

output "rds_cluster_endpoint" {
  description = "The cluster endpoint"
  value       = module.database_cluster.cluster_endpoint
}

output "database_url" {
  description = "The URL used to connect to the cluster"
  value       = "postgres://${module.database_cluster.cluster_endpoint}:${module.database_cluster.cluster_port}/${var.db_name}"
}
