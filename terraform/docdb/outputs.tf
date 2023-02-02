output "endpoint" {
  value = aws_docdb_cluster.docdb_primary.endpoint
}

output "username" {
  value = aws_docdb_cluster.docdb_primary.master_username
}

output "password" {
  value = aws_docdb_cluster.docdb_primary.master_password
}

output "port" {
  value = aws_docdb_cluster.docdb_primary.port
}

output "connection_url" {
  value = "mongodb://${aws_docdb_cluster.docdb_primary.master_username}:${aws_docdb_cluster.docdb_primary.master_password}@${aws_docdb_cluster.docdb_primary.endpoint}:${aws_docdb_cluster.docdb_primary.port}/${var.default_database}?tls=true&tlsCaFile=rds-combined-ca-bundle.pem&tlsAllowInvalidCertificates=true&replicaSet=rs0&readPreference=secondaryPreferred&retryWrites=false&minPoolSize=32&maxPoolSize=256&maxIdleTimeMS=30000&connectTimeoutMS=30000"
}

output "cluster_id" {
  value = aws_docdb_cluster.docdb_primary.cluster_identifier
}
