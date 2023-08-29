local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

local _configuration = defaults.configuration.timeseries
  .withUnit('ms')
  .withSoftLimit(
    axisSoftMin = 0,
    axisSoftMax = 0.8,
  );

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Notify Latency',
      datasource  = ds.prometheus,
    )
    .configure(_configuration)

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum(rate(notify_latency_sum[$__rate_interval])) / sum(rate(notify_latency_count[$__rate_interval]))',
      exemplar      = false,
      legendFormat  = 'Latency',
    ))
}
