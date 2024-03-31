# Terraform Configuration
terraform {
  required_version = ">= 1.0"

  backend "remote" {
    hostname     = "app.terraform.io"
    organization = "wallet-connect"
    workspaces {
      prefix = "notify-server-"
    }
  }

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 5.7"
    }
    grafana = {
      source  = "grafana/grafana"
      version = ">= 2.1"
    }
    random = {
      source  = "hashicorp/random"
      version = "3.6.0"
    }
  }
}
