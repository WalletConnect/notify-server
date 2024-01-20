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
        limit         = 70,
      )
    )

    .addTarget(targets.cloudwatch(
      datasource    = ds.cloudwatch,
      namespace     = 'AWS/RDS',
      metricName    = 'CPUUtilization',
      statistic     = 'Average',
      refId         = 'CPU_Avg'
    ))
}
