#-------------------------------------------------------------------------------
# DocDB Cluster

variable "db_port" {
  description = "The port the mongo database will listen on"
  type        = number
  default     = 27017
}

variable "engine" {
  type        = string
  default     = null
  description = "The name of the database engine to be used for this DB cluster. Defaults to `docdb`. Valid values: `docdb`"
}

variable "engine_version" {
  type        = string
  default     = null
  description = "The version number of the database engine to use"
}

variable "default_database" {
  description = "The name of the default database to create"
  type        = string
}

variable "master_username" {
  description = "The username for the master DB user"
  type        = string
  default     = "db_admin"
}

variable "master_password" {
  description = "The password for the master DB user"
  type        = string
  default     = ""
}

variable "enabled_cloudwatch_logs_exports" {
  type        = list(string)
  description = "List of log types to export to cloudwatch. The following log types are supported: `audit`, `profiler`"
  default     = []

  validation {
    condition     = length([for v in var.enabled_cloudwatch_logs_exports : v if contains(["audit", "profiler"], v)]) == length(var.enabled_cloudwatch_logs_exports)
    error_message = "Invalid log type"
  }
}

#-------------------------------------------------------------------------------
# Instance

variable "primary_instance_count" {
  description = "The number of primary instances to create"
  type        = number
}

variable "primary_instance_class" {
  description = "The instance class of the primary instances"
  type        = string
}

variable "replica_instance_count" {
  description = "The number of replica instances to create"
  type        = number
}

variable "replica_instance_class" {
  description = "The instance class of the replica instances"
  type        = string
}

variable "enable_performance_insights" {
  description = "Enable performance insights"
  type        = bool
  default     = false
}

#-------------------------------------------------------------------------------
# Networking

variable "vpc_id" {
  description = "The ID of the VPC to deploy to"
  type        = string
}

variable "subnet_ids" {
  description = "The IDs of the subnets to deploy to"
  type        = list(string)
}

variable "allowed_security_groups" {
  type        = list(string)
  default     = []
  description = "List of existing Security Groups to be allowed to connect to the DocumentDB cluster"
}

variable "allow_ingress_from_self" {
  type        = bool
  default     = false
  description = "Adds the Document DB security group itself as a source for ingress rules. Useful when this security group will be shared with applications."
}

variable "allowed_cidr_blocks" {
  type        = list(string)
  default     = []
  description = "List of CIDR blocks to be allowed to connect to the DocumentDB cluster"
}

variable "egress_from_port" {
  type        = number
  default     = 0
  description = "[from_port]DocumentDB initial port range for egress (e.g. `0`)"
}

variable "egress_to_port" {
  type        = number
  default     = 0
  description = "[to_port]DocumentDB initial port range for egress (e.g. `65535`)"
}

variable "egress_protocol" {
  type        = string
  default     = "-1"
  description = "DocumentDB protocol for egress (e.g. `-1`, `tcp`)"
}

variable "allowed_egress_cidr_blocks" {
  type        = list(string)
  default     = ["0.0.0.0/0"]
  description = "List of CIDR blocks to be allowed to send traffic outside of the DocumentDB cluster"
}

#-------------------------------------------------------------------------------
# Backup & Snapshots

variable "retention_period" {
  type        = number
  default     = null
  description = "Number of days to retain backups for"
}

variable "preferred_backup_window" {
  type        = string
  default     = null
  description = "Daily time range during which the backups happen"
}

variable "skip_final_snapshot" {
  type        = bool
  description = "Determines whether a final DB snapshot is created before the DB cluster is deleted"
  default     = true
}

#-------------------------------------------------------------------------------
# Maintenance

variable "preferred_maintenance_window" {
  type        = string
  default     = null
  description = "The window to perform maintenance in. Syntax: `ddd:hh24:mi-ddd:hh24:mi`."
}

variable "apply_immediately" {
  type        = bool
  description = "Specifies whether any cluster modifications are applied immediately, or during the next maintenance window"
  default     = true
}

variable "deletion_protection" {
  type        = bool
  description = "A value that indicates whether the DB cluster has deletion protection enabled"
  default     = false
}
