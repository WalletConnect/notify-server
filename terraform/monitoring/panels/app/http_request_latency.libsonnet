local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

local _configuration = defaults.configuration.timeseries
  .withUnit('ms')
  .withSoftLimit(
    axisSoftMin = 0,
    axisSoftMax = 2000,
  );

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'HTTP Request Latency',
      datasource  = ds.prometheus,
    )
    .configure(_configuration)

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum(rate(http_request_latency_sum[$__rate_interval])) / sum(rate(http_request_latency_count[$__rate_interval]))',
      legendFormat  = '{{method}} {{endpoint}} r{{aws_ecs_task_revision}}',
      exemplar      = false,
      refId         = 'HttpRequestRate',
    ))
}
