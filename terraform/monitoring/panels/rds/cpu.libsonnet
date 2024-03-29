local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'CPU',
      datasource  = ds.cloudwatch,
    )
    .configure(defaults.configuration.timeseries)

    .setAlert(
      vars.environment,
      defaults.alerts.cpu(
        namespace     = vars.namespace,
        env           = vars.environment,
        title         = 'RDS',
        notifications = vars.notifications,
        refid         = 'CPU',
        limit         = 90,
        reducer       = grafana.alertCondition.reducers.Max,
      )
    )

    .addTarget(targets.cloudwatch(
      datasource    = ds.cloudwatch,
      namespace     = 'AWS/RDS',
      metricName    = 'CPUUtilization',
      dimensions  = {
        DBClusterIdentifier: vars.rds_cluster_id,
      },
      matchExact  = true,
      statistic     = 'Average',
      refId         = 'CPU_Avg'
    ))

    .addTarget(targets.cloudwatch(
      datasource    = ds.cloudwatch,
      namespace     = 'AWS/RDS',
      metricName    = 'CPUUtilization',
      dimensions  = {
        DBClusterIdentifier: vars.rds_cluster_id,
      },
      matchExact  = true,
      statistic     = 'Maximum',
      refId         = 'CPU_Max'
    ))
}
