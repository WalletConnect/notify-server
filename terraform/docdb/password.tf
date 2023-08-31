locals {
  master_password = var.master_password == "" ? random_password.master_password[0].result : var.master_password
}

resource "random_password" "master_password" {
  count   = var.master_password == "" ? 1 : 0
  length  = 16
  special = false
}

resource "aws_kms_key" "master_password" {
  description         = "KMS key for the ${module.this.id} DocumentDB cluster master password"
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
    ]
  })
}

resource "aws_kms_alias" "master_password" {
  name          = "alias/${module.this.id}-master-password"
  target_key_id = aws_kms_key.master_password.id
}

resource "aws_secretsmanager_secret" "master_password" {
  name       = "${module.this.id}-master-password"
  kms_key_id = aws_kms_key.master_password.arn
}

resource "aws_secretsmanager_secret_version" "master_password" {
  secret_id     = aws_secretsmanager_secret.master_password.id
  secret_string = local.master_password
}
