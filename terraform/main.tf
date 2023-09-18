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
  database_subnets = {
    "eu-central-1"   = ["10.1.7.0/24", "10.1.8.0/24", "10.1.9.0/24"]
    "us-east-1"      = ["10.2.7.0/24", "10.2.8.0/24", "10.2.9.0/24"]
    "ap-southeast-1" = ["10.3.7.0/24", "10.3.8.0/24", "10.3.9.0/24"]
  }

  environment = terraform.workspace

  # No geoip bucket for dev, use staging
  geoip_db_bucket_env             = local.environment == "dev" ? "staging" : local.environment
  geoip_db_bucket_name            = "${local.geoip_db_bucket_env}.relay.geo.ip.database.private.${local.geoip_db_bucket_env}.walletconnect"
  analytics_data_lake_bucket_name = "walletconnect.data-lake.${local.environment}"
}

module "dns" {
  source = "github.com/WalletConnect/terraform-modules.git//modules/dns" # tflint-ignore: terraform_module_pinned_source

  hosted_zone_name = var.public_url
  fqdn             = local.fqdn
}

resource "aws_prometheus_workspace" "prometheus" {
  alias = "prometheus-${terraform.workspace}-${local.app_name}"
}

data "aws_ecr_repository" "repository" {
  name = "notify-server"
}

# TODO rename to notify-docdb like on history: https://github.com/WalletConnect/gilgamesh/blob/102064e9cababd4908f30d7994ea149c5d05d95c/terraform/main.tf#L53
module "keystore-docdb" {
  source = "./docdb"

  app_name                    = local.app_name
  mongo_name                  = "keystore-docdb" # TODO use default?
  environment                 = terraform.workspace
  default_database            = "keystore" # TODO "notify"
  primary_instance_class      = var.docdb_primary_instance_class
  primary_instances           = var.docdb_primary_instances
  replica_instance_class      = var.docdb_replica_instance_class
  replica_instances           = var.docdb_replica_instances
  vpc_id                      = module.vpc.vpc_id
  private_subnet_ids          = module.vpc.private_subnets
  allowed_ingress_cidr_blocks = [module.vpc.vpc_cidr_block]
  allowed_egress_cidr_blocks  = [module.vpc.vpc_cidr_block]
}

module "postgres" {
  source = "./postgres"

  app_name    = local.app_name
  environment = terraform.workspace

  vpc_id                     = module.vpc.vpc_id
  private_subnets            = module.vpc.private_subnets
  vpc_cidr_block             = module.vpc.vpc_cidr_block
  database_subnet_group_name = module.vpc.database_subnet_group_name
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
  postgres_url        = module.postgres.postgres_url
  keypair_seed        = var.keypair_seed
  project_id          = var.project_id
  relay_url           = var.relay_url
  notify_url          = var.notify_url
  registry_url        = var.registry_url
  registry_auth_token = var.registry_auth_token

  telemetry_sample_ratio = local.environment == "prod" ? 0.25 : 1.0
  allowed_origins        = local.environment == "prod" ? "https://cloud.walletconnect.com" : "*"

  data_lake_bucket_name          = local.analytics_data_lake_bucket_name
  data_lake_kms_key_arn          = var.data_lake_kms_key_arn
  analytics_geoip_db_bucket_name = local.geoip_db_bucket_name
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

  create_database_subnet_group = true
  database_subnets             = local.database_subnets[var.region]

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
