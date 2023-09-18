module "database_cluster" {
  source  = "terraform-aws-modules/rds-aurora/aws"
  version = "8.3.1"

  name           = "${var.environment}-${var.app_name}-database"
  engine         = "aurora-postgresql"
  engine_version = "15.3"
  engine_mode    = "provisioned"
  instance_class = "db.serverless"
  instances = {
    1 = {}
  }

  master_username = "root"
  database_name   = "postgres"

  manage_master_user_password = false
  master_password             = random_password.master.result

  vpc_id               = var.vpc_id
  subnets              = var.private_subnets
  db_subnet_group_name = var.database_subnet_group_name
  security_group_rules = {
    vpc_ingress = {
      cidr_blocks = [var.vpc_cidr_block]
    }
  }

  storage_encrypted = true
  apply_immediately = true

  allow_major_version_upgrade = true

  monitoring_interval             = 30
  enabled_cloudwatch_logs_exports = ["postgresql"]

  serverlessv2_scaling_configuration = {
    min_capacity = 2
    max_capacity = 10
  }
}

resource "random_password" "master" {
  length  = 20
  special = false
}
