data "aws_caller_identity" "this" {}

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

moved {
  from = module.ecs.aws_kms_key.cloudwatch-logs
  to   = aws_kms_key.cloudwatch_logs
}

moved {
  from = module.ecs.aws_kms_alias.cloudwatch-logs
  to   = aws_kms_alias.cloudwatch_logs
}

resource "aws_kms_key" "cloudwatch_logs" {
  description         = "KMS key for encrypting CloudWatch Logs"
  enable_key_rotation = true
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid    = "Enable IAM User Permissions"
        Effect = "Allow"
        Principal = {
          AWS = data.aws_caller_identity.this.account_id
        }
        Action   = "kms:*"
        Resource = "*"
      },
      {
        Sid    = "AllowCloudWatchLogs"
        Effect = "Allow"
        Principal = {
          Service = "logs.${module.this.region}.amazonaws.com"
        }
        Action = [
          "kms:Encrypt*",
          "kms:Decrypt*",
          "kms:ReEncrypt*",
          "kms:GenerateDataKey*",
          "kms:Describe*"
        ]
        Resource = "*"
      },
    ]
  })
}

resource "aws_kms_alias" "cloudwatch_logs" {
  name          = "alias/${module.this.id}-cloudwatch-logs"
  target_key_id = aws_kms_key.cloudwatch_logs.key_id
}
