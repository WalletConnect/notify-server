locals {
  name_prefix     = replace("${var.environment}-${var.app_name}-${var.mongo_name}", "_", "-")
  master_password = aws_secretsmanager_secret_version.master_password.secret_string
}

resource "random_password" "master_password" {
  length  = 16
  special = false
}

#tfsec:ignore:aws-ssm-secret-use-customer-key
resource "aws_secretsmanager_secret" "master_password" {
  name = "${local.name_prefix}-master-password"
}

resource "aws_secretsmanager_secret_version" "master_password" {
  secret_id     = aws_secretsmanager_secret.master_password.id
  secret_string = random_password.master_password.result
}

resource "aws_kms_key" "docdb_encryption" {
  enable_key_rotation = true
}

resource "aws_docdb_cluster" "docdb_primary" {
  cluster_identifier              = "${local.name_prefix}-primary-cluster"
  master_username                 = "cast-server"
  master_password                 = local.master_password
  port                            = 27017
  db_subnet_group_name            = aws_docdb_subnet_group.private_subnets.name
  storage_encrypted               = true
  kms_key_id                      = aws_kms_key.docdb_encryption.arn
  enabled_cloudwatch_logs_exports = ["audit"]

  vpc_security_group_ids = [
    aws_security_group.service_security_group.id
  ]
  skip_final_snapshot = true
}

#tfsec:ignore:aws-documentdb-encryption-customer-key
resource "aws_docdb_cluster_instance" "docdb_instances" {
  count              = var.primary_instances
  identifier         = "${local.name_prefix}-primary-instance-${count.index}"
  cluster_identifier = aws_docdb_cluster.docdb_primary.id
  instance_class     = var.primary_instance_class
  promotion_tier     = 0
}

resource "aws_docdb_subnet_group" "private_subnets" {
  name       = "${local.name_prefix}-private-subnet-group"
  subnet_ids = var.private_subnet_ids
}

resource "aws_security_group" "service_security_group" {
  name        = "${local.name_prefix}-service"
  description = "Allow ingress from the application"
  vpc_id      = var.vpc_id

  ingress {
    description = "Allow inbound traffic to the DocDB cluster"
    from_port   = 27017
    to_port     = 27017
    protocol    = "TCP"
    cidr_blocks = var.allowed_ingress_cidr_blocks
  }

  egress {
    description = "Allow outbound traffic from the DocDB cluster"
    from_port   = 0    # Allowing any incoming port
    to_port     = 0    # Allowing any outgoing port
    protocol    = "-1" # Allowing any outgoing protocol
    cidr_blocks = var.allowed_egress_cidr_blocks
  }
}
