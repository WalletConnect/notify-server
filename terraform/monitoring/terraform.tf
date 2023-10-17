terraform {
  required_version = ">= 1.0"

  required_providers {
    grafana = {
      source  = "grafana/grafana"
      version = "~> 2.0"
    }
    jsonnet = {
      source  = "alxrem/jsonnet"
      version = "~> 2.2.0"
    }
  }
}

#provider "jsonnet" {
#  jsonnet_path = "./grafonnet-lib,./panels"
#}
