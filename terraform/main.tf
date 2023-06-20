module "label" {
  source  = "app.terraform.io/wallet-connect/label/null"
  version = "0.1.0"
}

locals {
  app_name = var.app_name
  fqdn     = terraform.workspace == "prod" ? var.public_url : "${terraform.workspace}.${var.public_url}"
  # latest_release_name = data.github_release.latest_release.name
  # version             = coalesce(var.image_version, substr(local.latest_release_name, 1, length(local.latest_release_name))) # tflint-ignore: terraform_unused_declarations


  cidr = {
    "eu-central-1"   = "10.1.0.0/16"
    "us-east-1"      = "10.2.0.0/16"
    "ap-southeast-1" = "10.3.0.0/16"
  }
  private_subnets = {
    "eu-central-1"   = ["10.1.1.0/24", "10.1.2.0/24", "10.1.3.0/24"]
    "us-east-1"      = ["10.2.1.0/24", "10.2.2.0/24", "10.2.3.0/24"]
    "ap-southeast-1" = ["10.3.1.0/24", "10.3.2.0/24", "10.3.3.0/24"]
  }
  public_subnets = {
    "eu-central-1"   = ["10.1.4.0/24", "10.1.5.0/24", "10.1.6.0/24"]
    "us-east-1"      = ["10.2.4.0/24", "10.2.5.0/24", "10.2.6.0/24"]
    "ap-southeast-1" = ["10.3.4.0/24", "10.3.5.0/24", "10.3.6.0/24"]
  }

  environment = terraform.workspace

  geoip_db_bucket_name = "${local.environment}.relay.geo.ip.database.private.${local.environment}.walletconnect"

}

module "analytics" {
  source      = "./analytics"
  app_name    = local.app_name
  environment = local.environment
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
  image               = "${data.aws_ecr_repository.repository.repository_url}:${var.image_version}"
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
  project_id          = var.project_id
  relay_url           = var.relay_url
  cast_url            = var.cast_url

  telemetry_sample_ratio = local.environment == "prod" ? 0.25 : 1.0
  allowed_origins        = local.environment == "prod" ? "https://cloud.walletconnect.com" : "*"

  analytics_datalake_bucket_name = module.analytics.bucket-name
  analytics_key_arn              = module.analytics.kms-key_arn
  analytics_geoip_db_bucket_name = local.geoip_db_bucket_name
  analytics_geoip_db_key         = var.geoip_db_key
}

module "vpc" {
  source  = "terraform-aws-modules/vpc/aws"
  version = "~> 3.0"

  name = "${var.environment}.${var.region}.${var.app_name}"
  cidr = local.cidr[var.region]

  azs                    = ["${var.region}a", "${var.region}b", "${var.region}c"]
  private_subnets        = local.private_subnets[var.region]
  public_subnets         = local.public_subnets[var.region]
  enable_dns_hostnames   = true
  enable_dns_support     = true
  enable_nat_gateway     = true
  single_nat_gateway     = false
  one_nat_gateway_per_az = true

  private_subnet_tags = {
    Visibility = "private"
    Class      = "private"
  }
  public_subnet_tags = {
    Visibility = "public"
    Class      = "public"
  }
}

module "o11y" {
  source = "./monitoring"

  app_name                = local.app_name
  prometheus_workspace_id = aws_prometheus_workspace.prometheus.id
  load_balancer_arn       = module.ecs.load_balancer_arn
  target_group_arn        = module.ecs.target_group_arn
  environment             = terraform.workspace
  docdb_cluster_id        = module.keystore-docdb.cluster_id
}
