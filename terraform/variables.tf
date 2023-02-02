variable "region" {
  type    = string
  default = "eu-central-1"
}

#variable "azs" {
#  type    = list(string)
#  default = ["eu-central-1a", "eu-central-1b", "eu-central-1c"]
#}

variable "public_url" {
  type    = string
  default = "cast.walletconnect.com"
}

variable "grafana_endpoint" {
  type = string
}

variable "image_version" {
  type    = string
  default = ""
}

variable "docdb_primary_instance_class" {
  type = string
}

variable "docdb_primary_instances" {
  type = number
}
