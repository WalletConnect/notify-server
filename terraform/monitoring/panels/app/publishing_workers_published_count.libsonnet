local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Published messages count',
      datasource  = ds.prometheus,
    )
    .configure(defaults.configuration.timeseries)
    .addTarget(targets.prometheus(
      datasource   = ds.prometheus,
      expr         = 'rate(publishing_queue_published_count_total{}[$__rate_interval])',
      legendFormat = "r{{aws_ecs_task_revision}}"
    ))
}
