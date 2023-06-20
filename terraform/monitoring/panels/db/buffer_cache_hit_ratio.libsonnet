local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

local _configuration = defaults.configuration.timeseries
  .withUnit('percent')
  .withSoftLimit(
    axisSoftMin = 90,
    axisSoftMax = 100,
  );

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Buffer Cache Hit Ratio',
      description = 'See https://docs.aws.amazon.com/documentdb/latest/developerguide/best_practices.html',
      datasource  = ds.cloudwatch,
    )
    .configure(_configuration)

    .addTarget(targets.cloudwatch(
      alias       = 'Average Cache Hit %',
      datasource  = ds.cloudwatch,
      namespace   = 'AWS/DocDB',
      metricName  = 'BufferCacheHitRatio',
      statistic   = 'Average',
      dimensions  = {
        DBClusterIdentifier: vars.docdb_cluster_id
      },
      matchExact  = true,
    ))
}
