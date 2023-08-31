data "aws_caller_identity" "current" {}

data "aws_iam_role" "rds_auth_role" {
  name = var.iam_db_role
}

resource "aws_db_subnet_group" "db_subnets" {
  name        = module.this.id
  description = "Subnet group for the ${module.this.id} RDS cluster"
  subnet_ids  = var.subnet_ids
}

module "database_cluster" {
  source  = "terraform-aws-modules/rds-aurora/aws"
  version = "8.3.1"

  name               = module.this.id
  database_name      = var.db_name
  engine             = "aurora-postgresql"
  engine_version     = "15.3"
  engine_mode        = "provisioned"
  ca_cert_identifier = "rds-ca-ecc384-g1"
  instance_class     = "db.serverless"
  instances          = { for i in range(1, var.instances + 1) : i => {} }

  master_username                     = var.db_master_username
  iam_database_authentication_enabled = true

  vpc_id               = var.vpc_id
  db_subnet_group_name = aws_db_subnet_group.db_subnets.name
  security_group_rules = {
    vpc_ingress = {
      cidr_blocks = var.ingress_cidr_blocks
    }
  }

  performance_insights_enabled = true
  storage_encrypted            = true
  allow_major_version_upgrade  = true
  apply_immediately            = true
  skip_final_snapshot          = true
  deletion_protection          = true

  monitoring_interval             = 30
  enabled_cloudwatch_logs_exports = ["postgresql"]

  serverlessv2_scaling_configuration = {
    min_capacity = var.min_capacity
    max_capacity = var.max_capacity
  }
}

resource "aws_iam_role_policy" "rds_auth_policy" {
  name = "${module.this.id}-rds-auth-policy"
  role = data.aws_iam_role.rds_auth_role.id
  policy = jsonencode({
    Version = "2012-10-17",
    Statement = [
      {
        Action = ["rds-db:connect"]
        Effect = "Allow"
        Resource = [
          "arn:aws:rds-db:${module.this.region}:${data.aws_caller_identity.current.account_id}:dbuser:${module.database_cluster.cluster_resource_id}/${var.db_master_username}"
        ]
      }
    ]
  })
}
