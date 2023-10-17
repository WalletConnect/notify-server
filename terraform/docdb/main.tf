data "aws_caller_identity" "this" {}

#-------------------------------------------------------------------------------
# KMS Keys

resource "aws_kms_key" "docdb_encryption" {
  description         = "KMS key for the ${module.this.id} DocumentDB cluster encryption"
  enable_key_rotation = true

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid    = "Enable IAM User Permissions"
        Effect = "Allow"
        Principal = {
          AWS = data.aws_caller_identity.this.account_id
        }
        Action   = "kms:*"
        Resource = "*"
      },
    ]
  })
}

resource "aws_kms_alias" "docdb_encryption" {
  name          = "alias/${module.this.id}-docdb-encryption"
  target_key_id = aws_kms_key.docdb_encryption.id
}

#-------------------------------------------------------------------------------
# DocDB Cluster

resource "aws_docdb_cluster" "main" {
  cluster_identifier              = module.this.id
  port                            = var.db_port
  engine                          = var.engine
  engine_version                  = var.engine_version
  master_username                 = var.master_username
  master_password                 = local.master_password
  storage_encrypted               = true
  kms_key_id                      = aws_kms_key.docdb_encryption.arn
  enabled_cloudwatch_logs_exports = var.enabled_cloudwatch_logs_exports

  backup_retention_period   = var.retention_period
  preferred_backup_window   = var.preferred_backup_window
  final_snapshot_identifier = lower(module.this.id)
  skip_final_snapshot       = var.skip_final_snapshot

  preferred_maintenance_window = var.preferred_maintenance_window
  apply_immediately            = var.apply_immediately
  deletion_protection          = var.deletion_protection

  db_subnet_group_name = aws_docdb_subnet_group.db_subnets.name
  vpc_security_group_ids = [
    aws_security_group.db_security_group.id
  ]
}

#tfsec:ignore:aws-documentdb-encryption-customer-key
resource "aws_docdb_cluster_instance" "primary" {
  count              = var.primary_instance_count
  identifier         = "${module.this.id}-primary-${count.index}"
  cluster_identifier = aws_docdb_cluster.main.id
  instance_class     = var.primary_instance_class
  promotion_tier     = 1

  enable_performance_insights     = var.enable_performance_insights
  performance_insights_kms_key_id = var.enable_performance_insights ? aws_kms_key.docdb_encryption.id : null

  preferred_maintenance_window = var.preferred_maintenance_window
  apply_immediately            = var.apply_immediately
}

#tfsec:ignore:aws-documentdb-encryption-customer-key
resource "aws_docdb_cluster_instance" "replica" {
  count              = var.replica_instance_count
  identifier         = "${module.this.id}-replica-${count.index}"
  cluster_identifier = aws_docdb_cluster.main.id
  instance_class     = var.replica_instance_class
  promotion_tier     = 0

  enable_performance_insights     = var.enable_performance_insights
  performance_insights_kms_key_id = var.enable_performance_insights ? aws_kms_key.docdb_encryption.id : null

  preferred_maintenance_window = var.preferred_maintenance_window
  apply_immediately            = var.apply_immediately
}
