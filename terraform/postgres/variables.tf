variable "app_name" {
  type = string
}

variable "environment" {
  type = string
}

variable "vpc_id" {
  type = string
}

variable "private_subnets" {
  type = set(string)
}

variable "vpc_cidr_block" {
  type = string
}

variable "database_subnet_group_name" {
  type = string
  sensitive = true
}
