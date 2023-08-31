output "endpoint" {
  description = "The connection endpoint"
  value       = aws_docdb_cluster.main.endpoint
}

output "username" {
  description = "The master username"
  value       = aws_docdb_cluster.main.master_username
}

output "password" {
  description = "The master password"
  value       = aws_docdb_cluster.main.master_password
}

output "port" {
  description = "The connection port"
  value       = aws_docdb_cluster.main.port
}

output "connection_url" {
  description = "The connection url"
  value       = "mongodb://${aws_docdb_cluster.main.master_username}:${aws_docdb_cluster.main.master_password}@${aws_docdb_cluster.main.endpoint}:${aws_docdb_cluster.main.port}/${var.default_database}?tls=true&tlsCaFile=rds-combined-ca-bundle.pem&tlsAllowInvalidCertificates=true&replicaSet=rs0&readPreference=secondaryPreferred&retryWrites=false&minPoolSize=32&maxPoolSize=256&maxIdleTimeMS=30000&connectTimeoutMS=30000"
}

output "cluster_id" {
  description = "The cluster identifier"
  value       = aws_docdb_cluster.main.cluster_identifier
}
