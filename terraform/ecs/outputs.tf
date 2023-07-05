output "service_name" {
  description = "The name of the service"
  value       = local.service_name
}

output "target_group_arn" {
  description = "The ARN of the target group"
  value       = aws_lb_target_group.target_group.arn
}

output "load_balancer_arn" {
  description = "The ARN of the load balancer"
  value       = aws_lb.application_load_balancer.arn
}

output "load_balancer_arn_suffix" {
  description = "The ARN suffix of the load balancer"
  value       = aws_lb.application_load_balancer.arn_suffix
}

