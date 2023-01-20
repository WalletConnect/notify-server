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
