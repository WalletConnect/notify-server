local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Freeable Memory',
      datasource  = ds.cloudwatch,
    )
    .configure(
      defaults.configuration.timeseries
      .withUnit(grafana.fieldConfig.units.DecBytes)
    )

    .setAlert(vars.environment, grafana.alert.new(
      namespace     = vars.namespace,
      name          = '%(env)s - RDS freeable memory low' % { env: vars.environment },
      message       = '%(env)s - RDS freeable memory low' % { env: vars.environment },
      notifications = vars.notifications,
      conditions    = [
        grafana.alertCondition.new(
          evaluatorParams = [ 30 ],
          evaluatorType   = 'lt',
          operatorType    = 'or',
          queryRefId      = 'Mem_Avg',
          queryTimeStart  = '5m',
          queryTimeEnd    = 'now',
          reducerType     = grafana.alert_reducers.Avg
        ),
      ],
    ))

    .addTarget(targets.cloudwatch(
      datasource    = ds.cloudwatch,
      namespace     = 'AWS/RDS',
      metricName    = 'FreeableMemory',
      statistic     = 'Average',
      refId         = 'Mem_Avg',
    ))
}
