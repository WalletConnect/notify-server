resource "aws_kms_key" "docdb_encryption" {
  description             = module.this.id_full
  enable_key_rotation     = true
  deletion_window_in_days = 10
}

resource "aws_kms_alias" "analytics_bucket" {
  target_key_id = aws_kms_key.docdb_encryption.id
  name          = "alias/docdb/${module.this.id}"
}


module "docdb-cluster" {
  source  = "app.terraform.io/wallet-connect/docdb-cluster/aws"
  version = "0.1.0"

  context = module.this.context

  kms_key_arn                 = aws_kms_key.docdb_encryption.arn
  vpc_id                      = data.aws_vpc.vpc.id
  subnet_ids                  = data.aws_subnets.private_subnets.ids
  allowed_ingress_cidr_blocks = [data.aws_vpc.vpc.cidr_block]
  allowed_egress_cidr_blocks  = [data.aws_vpc.vpc.cidr_block]
}
