local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Accounts Not Found',
      datasource  = ds.prometheus,
    )
    .configure(defaults.configuration.timeseries)

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum by(aws_ecs_task_revision) (increase(dispatched_notifications{type="not_found"}[5m]))',
      legendFormat  = 'r{{aws_ecs_task_revision}}',
      exemplar      = true,
      refId       = 'AccountsNotFound',
    ))
}
