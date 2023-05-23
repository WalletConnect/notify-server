module "this" {
  source  = "app.terraform.io/wallet-connect/label/null"
  version = "0.2.0"

  namespace = "walletconnect"
  stage     = terraform.workspace
  name      = local.app_name

  tags = {
    Application = local.app_name
  }
}
