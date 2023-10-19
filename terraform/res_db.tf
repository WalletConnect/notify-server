module "db_context" {
  source  = "app.terraform.io/wallet-connect/label/null"
  version = "0.3.2"
  context = module.this

  attributes = [
    "db"
  ]
}

module "docdb" {
  source     = "./docdb"
  context    = module.db_context
  attributes = ["docdb"]


  default_database       = module.this.name
  master_username        = "notifyserver"
  master_password        = var.docdb_master_password
  primary_instance_count = var.docdb_primary_instance_count
  primary_instance_class = var.docdb_primary_instance_class
  replica_instance_count = var.docdb_replica_instance_count
  replica_instance_class = var.docdb_replica_instance_class

  vpc_id                     = module.vpc.vpc_id
  subnet_ids                 = module.vpc.intra_subnets
  allowed_cidr_blocks        = [module.vpc.vpc_cidr_block]
  allowed_egress_cidr_blocks = [module.vpc.vpc_cidr_block]

  enabled_cloudwatch_logs_exports = ["audit"]
  deletion_protection             = true
}

module "postgres" {
  source     = "./postgres"
  context    = module.db_context
  attributes = ["postgres"]

  vpc_id              = module.vpc.vpc_id
  subnet_ids          = module.vpc.intra_subnets
  ingress_cidr_blocks = module.vpc.private_subnets_cidr_blocks

  cloudwatch_logs_key_arn = aws_kms_key.cloudwatch_logs.arn

  depends_on = [aws_iam_role.application_role]
}
