variable "mongo_name" {
  type = string
}

variable "app_name" {
  type = string
}

variable "environment" {
  type = string
}

variable "primary_instance_class" {
  type = string
}

variable "primary_instances" {
  type = number
}

variable "allowed_ingress_cidr_blocks" {
  type = list(string)
}

variable "allowed_egress_cidr_blocks" {
  type = list(string)
}

variable "private_subnet_ids" {
  type = list(string)
}

variable "vpc_id" {
  type = string
}

variable "default_database" {
  type = string
}
