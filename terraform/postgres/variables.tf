#-------------------------------------------------------------------------------
# Database configuration

variable "db_name" {
  description = "The name of the default database in the cluster"
  type        = string
  default     = "postgres"
}

variable "db_master_username" {
  description = "The username for the master DB user"
  type        = string
  default     = "pgadmin"
}

#-------------------------------------------------------------------------------
# Capacity

variable "instances" {
  description = "The number of database instances to create"
  type        = number
  default     = 1
}

variable "min_capacity" {
  description = "The minimum capacity for the Aurora cluster (in Aurora Capacity Units)"
  type        = number
  default     = 2
}

variable "max_capacity" {
  description = "The maximum capacity for the Aurora cluster (in Aurora Capacity Units)"
  type        = number
  default     = 10
}

#-------------------------------------------------------------------------------
# Security

variable "iam_db_role" {
  description = "The name of the IAM role that will be allowed to access the database"
  type        = string
}

#-------------------------------------------------------------------------------
# Networking

variable "vpc_id" {
  description = "The VPC ID to create the security group in"
  type        = string
}

variable "subnet_ids" {
  description = "The IDs of the subnets to deploy to"
  type        = list(string)
}

variable "ingress_cidr_blocks" {
  description = "The CIDR blocks to allow ingress from"
  type        = list(string)
}
