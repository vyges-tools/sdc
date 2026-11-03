create_clock -name clk -period 10 [get_ports clk]
set_data_check -from [get_ports din] -to [get_ports dout] 0.1
