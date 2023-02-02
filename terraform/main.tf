module "label" {
  source  = "app.terraform.io/wallet-connect/label/null"
  version = "0.1.0"
}

locals {
  app_name = "cast-server"
  fqdn     = terraform.workspace == "prod" ? var.public_url : "${terraform.workspace}.${var.public_url}"
  # latest_release_name = data.github_release.latest_release.name
  # version             = coalesce(var.image_version, substr(local.latest_release_name, 1, length(local.latest_release_name))) # tflint-ignore: terraform_unused_declarations
}

# TODO: Enable when a version is available
# data "github_release" "latest_release" {
#   repository  = local.app_name
#   owner       = "walletconnect"
#   retrieve_by = "latest"
# }

module "dns" {
  source = "github.com/WalletConnect/terraform-modules.git//modules/dns" # tflint-ignore: terraform_module_pinned_source

  hosted_zone_name = var.public_url
  fqdn             = local.fqdn
}

resource "aws_prometheus_workspace" "prometheus" {
  alias = "prometheus-${terraform.workspace}-${local.app_name}"
}

data "aws_ecr_repository" "repository" {
  name = "cast-server"
}

module "keystore-docdb" {
  source = "./docdb"

  app_name                    = local.app_name
  mongo_name                  = "keystore-docdb"
  environment                 = terraform.workspace
  default_database            = "keystore"
  primary_instance_class      = var.docdb_primary_instance_class
  primary_instances           = var.docdb_primary_instances
  vpc_id                      = module.vpc.vpc_id
  private_subnet_ids          = module.vpc.private_subnets
  allowed_ingress_cidr_blocks = [module.vpc.vpc_cidr_block]
  allowed_egress_cidr_blocks  = [module.vpc.vpc_cidr_block]
}



module "ecs" {
  source = "./ecs"

  app_name            = "${terraform.workspace}-${local.app_name}"
  prometheus_endpoint = aws_prometheus_workspace.prometheus.prometheus_endpoint
  image               = "${data.aws_ecr_repository.repository.repository_url}:${local.version}"
  acm_certificate_arn = module.dns.certificate_arn
  cpu                 = 512
  fqdn                = local.fqdn
  memory              = 1024
  private_subnets     = module.vpc.private_subnets
  public_subnets      = module.vpc.public_subnets
  region              = var.region
  route53_zone_id     = module.dns.zone_id
  vpc_cidr            = module.vpc.vpc_cidr_block
  vpc_id              = module.vpc.vpc_id
  mongo_address       = module.keystore-docdb.connection_url
  keypair_seed        = var.keypair_seed
}