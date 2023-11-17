local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Registry Request Latency',
      datasource  = ds.prometheus,
    )
    .configure(
      defaults.configuration.timeseries
        .withUnit('ms')
    )

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum by (aws_ecs_task_revision) (rate(registry_request_latency_sum[$__rate_interval])) / sum by (aws_ecs_task_revision) (rate(registry_request_latency_count[$__rate_interval]))',
      legendFormat  = 'r{{aws_ecs_task_revision}}',
      exemplar      = false,
      refId         = 'RegistryRequestLatency',
    ))
}
