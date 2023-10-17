resource "aws_docdb_subnet_group" "db_subnets" {
  name        = module.this.id
  description = "Subnet group for the ${module.this.id} DocumentDB cluster"
  subnet_ids  = var.subnet_ids
}

resource "aws_security_group" "db_security_group" {
  name        = module.this.id
  description = "Security Group for the ${module.this.id} DocumentDB cluster"
  vpc_id      = var.vpc_id
}

resource "aws_security_group_rule" "ingress_from_self" {
  count             = var.allow_ingress_from_self ? 1 : 0
  security_group_id = aws_security_group.db_security_group.id
  description       = "Allow traffic within the security group"
  type              = "ingress"
  from_port         = var.db_port
  to_port           = var.db_port
  protocol          = "TCP"
  self              = true
}

resource "aws_security_group_rule" "ingress_security_groups" {
  count                    = length(var.allowed_security_groups)
  security_group_id        = aws_security_group.db_security_group.id
  description              = "Allow inbound traffic from existing Security Groups"
  type                     = "ingress"
  from_port                = var.db_port
  to_port                  = var.db_port
  protocol                 = "TCP"
  source_security_group_id = element(var.allowed_security_groups, count.index)
}

resource "aws_security_group_rule" "ingress_cidr_blocks" {
  count             = length(var.allowed_cidr_blocks)
  security_group_id = aws_security_group.db_security_group.id
  description       = "Allow inbound traffic from CIDR blocks"
  type              = "ingress"
  from_port         = var.db_port
  to_port           = var.db_port
  protocol          = "TCP"
  cidr_blocks       = var.allowed_cidr_blocks
}

resource "aws_security_group_rule" "egress" {
  security_group_id = aws_security_group.db_security_group.id
  description       = "Allow outbound traffic from CIDR blocks"
  type              = "egress"
  from_port         = var.egress_from_port
  to_port           = var.egress_to_port
  protocol          = var.egress_protocol
  cidr_blocks       = var.allowed_egress_cidr_blocks
}
