module "redis_context" {
  source  = "app.terraform.io/wallet-connect/label/null"
  version = "0.3.2"
  context = module.this

  attributes = [
    "cache"
  ]
}

module "redis" {
  source  = "./redis"
  context = module.redis_context

  vpc_id      = module.vpc.vpc_id
  subnets_ids = module.vpc.intra_subnets
}
