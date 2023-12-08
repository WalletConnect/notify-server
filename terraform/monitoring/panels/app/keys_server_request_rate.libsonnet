local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Keys Server Req Rate',
      datasource  = ds.prometheus,
    )
    .configure(
      defaults.configuration.timeseries
        .withUnit('cps')
    )

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum by (aws_ecs_task_revision) (rate(keys_server_requests_total{source="server"}[$__rate_interval]))',
      legendFormat  = 'Server r{{aws_ecs_task_revision}}',
      exemplar      = true,
      refId         = 'KeysServerRequestRateServer',
    ))

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum by (aws_ecs_task_revision) (rate(keys_server_requests_total{source="cache"}[$__rate_interval]))',
      legendFormat  = 'Cache r{{aws_ecs_task_revision}}',
      exemplar      = true,
      refId         = 'KeysServerRequestRateCache',
    ))

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum by (aws_ecs_task_revision) (rate(keys_server_requests_total[$__rate_interval]))',
      legendFormat  = 'Total r{{aws_ecs_task_revision}}',
      exemplar      = true,
      refId         = 'KeysServerRequestRateTotal',
    ))
}
