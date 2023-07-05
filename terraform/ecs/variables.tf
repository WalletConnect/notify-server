variable "region" {
  type = string
}

variable "app_name" {
  type = string
}

variable "image" {
  type = string
}

variable "prometheus_endpoint" {
  type = string
}

variable "vpc_id" {
  type = string
}

variable "vpc_cidr" {
  type = string
}

variable "mongo_address" {
  type = string
}

variable "keypair_seed" {
  type = string
}

variable "route53_zone_id" {
  type = string
}

variable "fqdn" {
  type = string
}

variable "acm_certificate_arn" {
  type = string
}

variable "public_subnets" {
  type = set(string)
}

variable "private_subnets" {
  type = set(string)
}

variable "cpu" {
  type = number
}

variable "memory" {
  type = number
}


variable "analytics_geoip_db_bucket_name" {
  description = "The name of the bucket where the geoip database is stored"
  type        = string
}

variable "telemetry_sample_ratio" {
  type = number
}

variable "allowed_origins" {
  type = string
}

variable "project_id" {
  type = string
}

variable "relay_url" {
  type = string
}

variable "cast_url" {
  type = string
}

variable "data_lake_bucket_name" {
  description = "The name of the data-lake bucket."
  type        = string
}

variable "data_lake_kms_key_arn" {
  description = "The ARN of the KMS encryption key for data-lake bucket."
  type        = string
}

variable "geoip_db_key" {
  description = "The key to the GeoIP database"
  type        = string
  default     = "GeoLite2-City.mmdb"
}