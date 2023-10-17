resource "random_pet" "this" {
  length = 2
}

locals {
  ecr_repository_url = data.terraform_remote_state.org.outputs.accounts.wl.notify[local.stage].ecr-url

  stage = lookup({
    "notify-server-wl-staging" = "staging",
    "notify-server-wl-prod"    = "prod",
    "notify-server-staging"    = "staging",
    "notify-server-prod"       = "prod",
    "wl-staging"               = "staging",
    "wl-prod"                  = "prod",
    "staging"                  = "staging",
    "prod"                     = "prod",
  }, terraform.workspace, terraform.workspace)
}
